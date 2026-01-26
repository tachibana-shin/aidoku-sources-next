#![no_std]
use aidoku::{
	Chapter, FilterValue, Manga, Page, PageContent, Result, Source, Viewer,
	alloc::{borrow::ToOwned, string::ToString, *},
	helpers::uri::QueryParameters,
	imports::{
		html::{Element, Html},
		std::send_partial_result,
	},
	prelude::*,
};
use wpcomics::{Cache, Impl, Params, WpComics, helpers::extract_f32_from_string};

const BASE_URL: &str = "https://novelfull.net";

fn remove_node(node: Element, content_html: &mut String) {
	if let Some(node_html) = node.outer_html() {
		*content_html = content_html.replace(&node_html, "");
	}
}

struct NovelFull;

impl Impl for NovelFull {
	fn new() -> Self {
		Self
	}

	fn params(&self) -> Params {
		Params {
			base_url: BASE_URL.into(),
			viewer: Viewer::RightToLeft,

			next_page: ".pagination .last:not(.disabled)",
			manga_cell: ".list-truyen > .row:has(.text-info)",
			manga_cell_title: ".truyen-title a",
			manga_cell_url: ".truyen-title a",
			manga_cell_image: "img",
			manga_cell_image_attr: "abs:src",
			manga_parse_id: |url| {
				url.split("/")
					.last()
					.unwrap_or_default()
					.trim_end_matches(".html")
					.to_string()
			},
			chapter_parse_id: |url| {
				url.split("/")
					.last()
					.unwrap_or_default()
					.trim_end_matches(".html")
					.to_string()
			},

			manga_details_title: "h3.title",
			manga_details_cover: ".book img",
			manga_details_cover_attr: "abs:src",
			manga_details_authors: "h3:contains(Author:) + a",
			manga_details_description: ".desc-text",
			manga_details_tags: "h3:contains(Genre:) + a",
			manga_details_tags_splitter: "",
			manga_details_status: "h3:contains(Status:) + a",

			manga_details_chapters: ".list-chapter > li",
			chapter_anchor_selector: "a",

			manga_page: |_, manga| format!("{BASE_URL}/{}.html", manga.key),
			page_list_page: |_, manga, chapter| {
				format!("{BASE_URL}/{}/{}.html", manga.key, chapter.key)
			},
			get_search_url: |params, q, page, filters| {
				let mut query = QueryParameters::new();
				query.push("keyword", Some(&q.to_owned().unwrap_or_default()));
				query.push("page", Some(&page.to_string()));

				for filter in filters {
					match filter {
						FilterValue::Select { value, .. } => {
							return Ok(format!("{}/genre/{}", BASE_URL, value));
						}
						_ => {}
					}
				}

				return Ok(format!("{}/search?{query}", params.base_url));
			},

			home_manga_link: "a",

			home_grids_selector: "#intro-index, #truyen-slide",
			home_grids_title_selector: "h2 > a",
			home_grids_item_selector: ".index-intro > .item, .row > .col-xs-4",
			home_manga_cover_attr: "abs:src",

			..Default::default()
		}
	}

	fn get_home(&self, cache: &mut wpcomics::Cache, params: &Params) -> Result<aidoku::HomeLayout> {
		let base_url = &params.base_url.clone();
		let html = self.create_request(cache, params, base_url, None)?.html()?;

		let mut components = Vec::new();

		let parse_manga = |el: &Element, slider: bool| -> Option<Manga> {
			let manga_link = el
				.select_first(params.home_manga_link)
				.or_else(|| el.select_first(".widget-title a"))?;
			let cover = el
				.select_first(params.home_manga_cover_selector)
				.and_then(|img| {
					img.attr(if slider {
						params
							.home_manga_cover_slider_attr
							.unwrap_or(params.home_manga_cover_attr)
					} else {
						params.home_manga_cover_attr
					})
					.or_else(|| img.attr("data-cfsrc"))
				})
				.map(|src| {
					if slider {
						(params.home_manga_cover_slider_transformer)(src)
					} else {
						src
					}
				});
			let url = manga_link.attr("abs:href")?;

			Some(Manga {
				key: (params.manga_parse_id)(&url),
				title: manga_link.text()?,
				cover,
				url: Some(url),
				..Default::default()
			})
		};

		if let Some(main_cols) = html.select(params.home_grids_selector) {
			for (idx, main_col) in main_cols.enumerate() {
				let title = main_col
					.select_first(params.home_grids_title_selector)
					.and_then(|el| el.text());
				let last_updates = main_col
					.select(params.home_grids_item_selector)
					.map(|els| {
						els.filter_map(|el| parse_manga(&el, false).map(|v| v.into()))
							.collect::<Vec<_>>()
					})
					.unwrap_or_default();

				if !last_updates.is_empty() {
					components.push(aidoku::HomeComponent {
						title,
						subtitle: None,
						value: aidoku::HomeComponentValue::MangaList {
							page_size: Some(4),
							entries: last_updates,
							listing: None,
							ranking: idx == 0,
						},
					});
				}
			}
		}

		Ok(aidoku::HomeLayout { components })
	}

	fn get_page_list(
		&self,
		cache: &mut wpcomics::Cache,
		params: &Params,
		manga: Manga,
		chapter: Chapter,
	) -> Result<Vec<Page>> {
		let mut pages: Vec<Page> = Vec::new();

		let url = (params.page_list_page)(params, &manga, &chapter);
		let html = self.create_request(cache, params, &url, None)?.html()?;

		let Some(content) = html.select_first("#chr-content, #chapter-content") else {
			bail!("Failed to get chapter content");
		};

		if let Some(scripts) = content.select("script, .unlock-buttons, #frame, iframe") {
			scripts.remove();
		}

		let Some(mut content_html) = content.html() else {
			bail!("Failed to get chapter content HTML");
		};

		if let Some(styles_node) = content.select("[style]") {
			for style_node in styles_node {
				if let Some(style) = style_node.attr("style") {
					let has_display_none = style.contains("display:")
						&& style[style.find("display:").unwrap_or_default()..].contains("none");
					if has_display_none {
						remove_node(style_node, &mut content_html);
					}
				}
			}
		}

		// remove comments
		while let Some(start) = content_html.find("<!--") {
			if let Some(end) = content_html[start..].find("-->") {
				let end_pos = start + end + 3;
				content_html.drain(start..end_pos);
			} else {
				break;
			}
		}

		// end modify html

		let description = html
			.select_first(".chr-title")
			.and_then(|v| v.attr("title"));

		pages.push(Page {
			content: PageContent::Text(format!("<!--html-->{content_html}")),
			has_description: description.is_some(),
			description,
			..Default::default()
		});

		Ok(pages)
	}

	fn get_chapter_list(
		&self,
		cache: &mut Cache,
		params: &Params,
		url: String,
	) -> Result<Vec<Chapter>> {
		let mut all_chapters = Vec::new();
		let mut page = 1;

		loop {
			let html_data =
				self.cache_manga_page(cache, params, &format!("{}?page={page}", url.to_owned()))?;
			let html = Html::parse_with_url(html_data, url.to_owned())?;
			let title_untrimmed = (params.manga_details_title_transformer)(
				html.select(params.manga_details_title)
					.and_then(|v| v.text())
					.unwrap_or_default(),
			);
			let title = title_untrimmed.trim();
			let mut skipped_first = false;

			let Some(chapters_iter) = html.select(params.manga_details_chapters) else {
				return Ok(Vec::new());
			};

			let chapters = chapters_iter
				.filter_map(|chapter_node| {
					if params.chapter_skip_first && !skipped_first {
						skipped_first = true;
						return None;
					}

					let chapter_url = chapter_node
						.select_first(params.chapter_anchor_selector)?
						.attr("abs:href")?;

					let chapter_id = (params.chapter_parse_id)(chapter_url.clone());
					let raw_chapter_title = chapter_node
						.select(params.chapter_anchor_selector)?
						.text()
						.unwrap_or_default();
					let numbers = extract_f32_from_string(title, &raw_chapter_title);
					let (volume_number, chapter_number) = if numbers.len() > 1
						&& raw_chapter_title.to_ascii_lowercase().contains("vol")
					{
						(numbers[0], numbers[1])
					} else if !numbers.is_empty() {
						(-1.0, numbers[0])
					} else {
						(-1.0, -1.0)
					};
					let mut new_chapter_title = None;
					if chapter_number >= 0.0 {
						let splitter = format!(" {}", chapter_number);
						let splitter2 = format!("#{}", chapter_number);
						if raw_chapter_title.contains(&splitter) {
							let split = raw_chapter_title
								.splitn(2, &splitter)
								.collect::<Vec<&str>>();
							new_chapter_title =
								Some(String::from(split[1]).replacen([':', '-'], "", 1));
						} else if raw_chapter_title.contains(&splitter2) {
							let split = raw_chapter_title
								.splitn(2, &splitter2)
								.collect::<Vec<&str>>();
							new_chapter_title =
								Some(String::from(split[1]).replacen([':', '-'], "", 1));
						}
					}
					let date_updated = (params.time_converter)(
						params,
						&chapter_node
							.select(params.chapter_date_selector)?
							.text()
							.unwrap_or_default(),
					);

					let chapter_title = new_chapter_title
						.and_then(|s| {
							let trimmed = s.trim();
							if trimmed.is_empty() {
								None
							} else {
								Some(trimmed.into())
							}
						})
						.or_else(|| Some(raw_chapter_title.trim().into()));

					Some(Chapter {
						key: chapter_id,
						title: chapter_title,
						volume_number: if volume_number < 0.0 {
							None
						} else {
							Some(volume_number)
						},
						chapter_number: if chapter_number < 0.0 && volume_number >= 0.0 {
							None
						} else {
							Some(chapter_number)
						},
						date_uploaded: Some(date_updated),
						url: Some(chapter_url),
						..Default::default()
					})
				})
				.collect::<Vec<_>>();

			all_chapters.extend(chapters);

			if html
				.select_first(".pagination .last:not(.disabled)")
				.is_none()
			{
				break;
			}

			page += 1;
		}

		Ok(all_chapters)
	}

	fn get_manga_update(
		&self,
		cache: &mut wpcomics::Cache,
		params: &Params,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		let url = (params.manga_page)(params, &manga);

		if needs_details {
			let new_manga = self.parse_manga_element(cache, params, url.clone())?;

			manga.copy_from(new_manga);

			if needs_chapters {
				send_partial_result(&manga);
			}
		}

		if needs_chapters {
			let mut chapters = self.get_chapter_list(cache, params, url)?;
			chapters.reverse();

			manga.chapters = Some(chapters);
		}

		Ok(manga)
	}
}

register_source!(
	WpComics<NovelFull>,
	ImageRequestProvider,
	DeepLinkHandler,
	Home
);
