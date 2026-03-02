use crate::{BASE_URL, USER_AGENT};
use aidoku::{
	Chapter, ContentRating, Manga, MangaPageResult, MangaStatus, Page, PageContent, Result,
	SelectFilter, Viewer,
	alloc::{String, Vec, borrow::Cow, string::ToString as _},
	imports::{html::Document, net::Request},
	prelude::*,
};

pub trait MangaListPage {
	fn manga_page_result(&self) -> Result<MangaPageResult>;
}

pub trait MangaDetailPage {
	fn manga_details(&self, url: String, key: String) -> Result<Manga>;
}

pub trait TagsPage {
	fn tags_filter(&self) -> Result<SelectFilter>;
}

impl MangaListPage for Document {
	fn manga_page_result(&self) -> Result<MangaPageResult> {
		let mut mangas: Vec<Manga> = Vec::new();

		let items = self
			.select(".common-comic-item")
			.ok_or_else(|| error!("No manga items found"))?;

		for item in items {
			let a_node = match item.select_first("a.cover") {
				Some(n) => n,
				None => continue,
			};
			let href = a_node.attr("abs:href").unwrap_or_default();
			let key = href
				.trim_end_matches('/')
				.split('/')
				.next_back()
				.unwrap_or_default()
				.to_string();
			if key.is_empty() {
				continue;
			}

			let cover = item
				.select_first("img.lazy")
				.and_then(|img| img.attr("data-original"));

			let title = item
				.select_first("p.comic__title")
				.and_then(|p| p.select_first("a"))
				.and_then(|a| a.text())
				.unwrap_or_default();

			mangas.push(Manga {
				key,
				cover,
				title,
				..Default::default()
			});
		}

		let has_next_page = self.select_first("a.next").is_some();

		Ok(MangaPageResult {
			entries: mangas,
			has_next_page,
		})
	}
}

impl MangaDetailPage for Document {
	fn manga_details(&self, url: String, key: String) -> Result<Manga> {
		let cover = self
			.select_first(".de-info__cover img")
			.and_then(|img| img.attr("src"));

		let title = self
			.select_first("p.comic-title.j-comic-title")
			.and_then(|n| n.text())
			.unwrap_or_default();

		let author_raw = self
			.select_first(".comic-author .name a")
			.and_then(|n| n.text())
			.unwrap_or_default();
		let authors: Vec<String> = author_raw
			.split('&')
			.map(|a| a.trim().to_string())
			.filter(|a| !a.is_empty())
			.collect();

		let description = self
			.select_first(".comic-intro p.intro-total")
			.and_then(|n| n.text())
			.map(|s| s.trim().to_string());

		let tags: Vec<String> = self
			.select(".comic-status .text b a")
			.map(|nodes| nodes.filter_map(|n| n.text()).collect())
			.unwrap_or_default();

		let status_text = self
			.select_first(".de-chapter__title span")
			.and_then(|n| n.text())
			.unwrap_or_default();
		let status = if status_text.contains("完结") {
			MangaStatus::Completed
		} else {
			MangaStatus::Ongoing
		};

		Ok(Manga {
			key,
			cover,
			title,
			authors: Some(authors),
			description,
			url: Some(url),
			tags: if tags.is_empty() { None } else { Some(tags) },
			status,
			content_rating: ContentRating::NSFW,
			viewer: Viewer::Webtoon,
			..Default::default()
		})
	}
}

pub fn parse_chapter_list(doc: &Document) -> Result<Vec<Chapter>> {
	let mut chapters: Vec<Chapter> = Vec::new();

	let items = match doc.select("li.j-chapter-item") {
		Some(v) => v,
		None => return Ok(chapters),
	};

	for (index, item) in items.enumerate() {
		let a_node = match item.select_first("a.j-chapter-link") {
			Some(n) => n,
			None => continue,
		};
		let href = a_node.attr("href").unwrap_or_default();
		let key = href
			.trim_end_matches('/')
			.split('/')
			.next_back()
			.unwrap_or_default()
			.to_string();
		if key.is_empty() {
			continue;
		}
		let title = a_node
			.text()
			.map(|s| s.trim().to_string())
			.filter(|s| !s.is_empty());
		let chapter_number = (index + 1) as f32;

		chapters.push(Chapter {
			key,
			title,
			chapter_number: Some(chapter_number),
			url: if href.is_empty() {
				None
			} else {
				Some(href.to_string())
			},
			..Default::default()
		});
	}

	chapters.reverse();
	Ok(chapters)
}

impl TagsPage for Document {
	fn tags_filter(&self) -> Result<SelectFilter> {
		let links = self
			.select(".cate-item a")
			.ok_or_else(|| error!("Failed to select tag links"))?;

		let (mut options, mut ids): (Vec<Cow<str>>, Vec<Cow<str>>) = links
			.filter_map(|a| {
				let href = a.attr("href")?;
				let tags_pos = href.find("/tags/")?;
				let id = href[tags_pos + "/tags/".len()..]
					.trim_end_matches('/')
					.split('/')
					.next()?
					.to_string();
				if id.is_empty() {
					return None;
				}
				let name = a.text()?.trim().to_string();
				if name.is_empty() {
					return None;
				}
				Some((Cow::Owned(name), Cow::Owned(id)))
			})
			.unzip();

		options.insert(0, Cow::Borrowed("全部"));
		ids.insert(0, Cow::Borrowed(""));

		Ok(SelectFilter {
			id: "标签".into(),
			title: Some("标签".into()),
			is_genre: true,
			options,
			ids: Some(ids),
			..Default::default()
		})
	}
}

pub fn get_page_list(chapter_key: &str) -> Result<Vec<Page>> {
	let url = format!("{}/chapter/{}", BASE_URL, chapter_key);
	let html = Request::get(url)?.header("User-Agent", USER_AGENT).html()?;
	let mut pages: Vec<Page> = Vec::new();

	if let Some(imgs) = html.select("img.lazy-read") {
		for img in imgs {
			let img_url = img
				.attr("data-original")
				.map(|s| s.trim().to_string())
				.unwrap_or_default();
			if img_url.is_empty() {
				continue;
			}
			pages.push(Page {
				content: PageContent::url(img_url),
				..Default::default()
			});
		}
	}

	Ok(pages)
}
