#![no_std]
use aidoku::{
	Chapter, DeepLinkHandler, DeepLinkResult, FilterValue, Listing, ListingProvider, Manga,
	MangaPageResult, Page, PageContent, Result, Source,
	alloc::{String, Vec, string::ToString, vec},
	helpers::uri::encode_uri_component,
	imports::{error::AidokuError, net::Request},
	prelude::*,
};

mod home;
mod models;
mod settings;

use core::cell::RefCell;

use models::*;

const BASE_URL: &str = "https://nhentai.net";
const API_URL: &str = "https://nhentai.net/api/v2";
const USER_AGENT: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_2 like Mac OS X) \
						  AppleWebKit/605.1.15 (KHTML, like Gecko) GSA/300.0.598994205 \
						  Mobile/15E148 Safari/604";

struct NHentai {
	cache: RefCell<Option<(String, NHentaiGallery)>>,
}

impl Source for NHentai {
	fn new() -> Self {
		Self {
			cache: RefCell::new(None),
		}
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		// If the query is a numeric ID, return the manga directly
		if let Some(q) = &query
			&& let Ok(id) = q.parse::<i32>()
		{
			let url = format!("{API_URL}/galleries/{id}");
			let gallery: NHentaiGallery = Request::get(&url)?
				.header("User-Agent", USER_AGENT)
				.json_owned()?;
			return Ok(MangaPageResult {
				entries: vec![gallery.into()],
				has_next_page: false,
			});
		}

		let mut query_parts = Vec::new();

		if let Some(q) = query {
			query_parts.push(q);
		}

		let mut sort = "date";

		// parse filters
		for filter in filters {
			match filter {
				FilterValue::Text { id, value } => match id.as_str() {
					"author" => {
						query_parts.push(value);
					}
					"artist" => {
						query_parts.push(format!("artist:{value}"));
					}
					"groups" => {
						query_parts.push(format!("group:{value}"));
					}
					_ => continue,
				},
				FilterValue::Sort { index, .. } => {
					sort = match index {
						0 => "date",          // Latest
						1 => "popular-today", // Popular Today
						2 => "popular-week",  // Popular Week
						3 => "popular",       // Popular All
						_ => "date",
					};
				}
				FilterValue::MultiSelect {
					id,
					included,
					excluded,
					..
				} => {
					if id == "tags" {
						for tag in included {
							query_parts.push(format!("tag:\"{tag}\""));
						}
						for tag in excluded {
							query_parts.push(format!("-tag:\"{tag}\""));
						}
					}
				}
				FilterValue::Select { id, value } => {
					if id == "genre" {
						query_parts.push(format!("tag:\"{value}\""));
					}
				}
				_ => continue,
			}
		}

		if let Some(language) = settings::get_language() {
			query_parts.push(format!("language:{language}"));
		}

		for blocked in settings::get_blocklist() {
			if !blocked.is_empty() {
				query_parts.push(format!("-tag:\"{blocked}\""));
			}
		}

		let combined_query = if query_parts.is_empty() {
			" ".into()
		} else {
			query_parts.join(" ")
		};

		let url = format!(
			"{API_URL}/search?query={}&page={page}&sort={sort}",
			encode_uri_component(combined_query),
		);
		let response: NHentaiSearchResponse = Request::get(&url)?
			.header("User-Agent", USER_AGENT)
			.json_owned()?;

		let entries = response
			.result
			.into_iter()
			.map(|item| item.into())
			.collect::<Vec<Manga>>();
		let has_next_page = page < response.num_pages;

		Ok(MangaPageResult {
			entries,
			has_next_page,
		})
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		if needs_details || needs_chapters {
			let url = format!("{API_URL}/galleries/{}", manga.key);
			let gallery: NHentaiGallery = Request::get(&url)?
				.header("User-Agent", USER_AGENT)
				.json_owned()?;

			if needs_details {
				manga.copy_from(gallery.clone().into());
			}

			if needs_chapters {
				let mut languages = Vec::new();
				for tag in &gallery.tags {
					if tag.r#type == "language" && tag.name != "translated" && tag.name != "rewrite"
					{
						languages.push(tag.name.clone());
					}
				}

				let chapter = Chapter {
					key: manga.key.clone(),
					chapter_number: Some(1.0),
					date_uploaded: Some(gallery.upload_date),
					url: Some(format!("{}/g/{}", BASE_URL, manga.key)),
					scanlators: if !languages.is_empty() {
						Some(vec![languages.join(", ")])
					} else {
						None
					},
					..Default::default()
				};
				manga.chapters = Some(vec![chapter]);
			}

			// Cache the fetched gallery for potential reuse in get_page_list
			self.cache
				.borrow_mut()
				.replace((manga.key.clone(), gallery));
		}

		Ok(manga)
	}

	fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		// Try to reuse cached gallery fetched by get_manga_update
		let maybe_cached = self.cache.borrow();
		let gallery: NHentaiGallery = match &*maybe_cached {
			Some((cached_key, cached_gallery)) if cached_key == &chapter.key => {
				cached_gallery.clone()
			}
			_ => {
				let api_url = format!("{API_URL}/galleries/{}", chapter.key);
				Request::get(&api_url)?
					.header("User-Agent", USER_AGENT)
					.json_owned()?
			}
		};

		let pages = gallery
			.pages
			.iter()
			.map(|page| {
				let path = make_image_url(&page.path, false);

				Page {
					content: PageContent::url(path),
					..Default::default()
				}
			})
			.collect::<Vec<Page>>();

		Ok(pages)
	}
}

impl ListingProvider for NHentai {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		match listing.id.as_str() {
			"popular-today" => self.get_search_manga_list(
				None,
				page,
				vec![FilterValue::Sort {
					id: "sort".to_string(),
					index: 1,
					ascending: false,
				}],
			),
			"popular-week" => self.get_search_manga_list(
				None,
				page,
				vec![FilterValue::Sort {
					id: "sort".to_string(),
					index: 2,
					ascending: false,
				}],
			),
			"popular" => self.get_search_manga_list(
				None,
				page,
				vec![FilterValue::Sort {
					id: "sort".to_string(),
					index: 3,
					ascending: false,
				}],
			),
			"latest" => self.get_search_manga_list(
				None,
				page,
				vec![FilterValue::Sort {
					id: "sort".to_string(),
					index: 0,
					ascending: false,
				}],
			),
			_ => Err(AidokuError::Unimplemented),
		}
	}
}

impl DeepLinkHandler for NHentai {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		if !url.starts_with(BASE_URL) {
			return Ok(None);
		}

		const GALLERY_PATH: &str = "/g/";

		if let Some(id_start) = url.find(GALLERY_PATH) {
			let id_part = &url[id_start + GALLERY_PATH.len()..];
			let end = id_part.find('/').unwrap_or(id_part.len());
			let manga_id = &id_part[..end];

			Ok(Some(DeepLinkResult::Manga {
				key: manga_id.into(),
			}))
		} else {
			Ok(None)
		}
	}
}

register_source!(NHentai, Home, ListingProvider, DeepLinkHandler);
