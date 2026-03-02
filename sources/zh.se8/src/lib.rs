#![no_std]

mod html;
mod net;

use aidoku::{
	Chapter, DeepLinkHandler, DeepLinkResult, DynamicFilters, Filter, FilterValue,
	ImageRequestProvider, Manga, MangaPageResult, Page, PageContext, Result, Source,
	alloc::{String, Vec, string::ToString as _},
	imports::net::Request,
	prelude::*,
};
use html::{MangaDetailPage as _, MangaListPage as _, TagsPage as _, parse_chapter_list};
use net::Url;

pub const BASE_URL: &str = "https://se8.us/index.php";
pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";

struct Se8;

impl Source for Se8 {
	fn new() -> Self {
		Self
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		let url = Url::from_query_or_filters(query.as_deref(), page, &filters)?;
		let html = url.request()?.html()?;
		html.manga_page_result()
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		let detail_url = format!("{}/comic/{}", BASE_URL, manga.key);

		let html = Request::get(&detail_url)?
			.header("User-Agent", USER_AGENT)
			.html()?;

		if needs_details {
			manga = html.manga_details(detail_url, manga.key.clone())?;
		}

		if needs_chapters {
			manga.chapters = Some(parse_chapter_list(&html)?);
		}

		Ok(manga)
	}

	fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		html::get_page_list(&chapter.key)
	}
}

impl ImageRequestProvider for Se8 {
	fn get_image_request(&self, url: String, _context: Option<PageContext>) -> Result<Request> {
		Ok(Request::get(url)?.header("User-Agent", USER_AGENT))
	}
}

impl DeepLinkHandler for Se8 {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		if url.contains("/comic/") {
			let key = url
				.trim_end_matches('/')
				.split('/')
				.next_back()
				.unwrap_or_default()
				.to_string();
			if !key.is_empty() {
				return Ok(Some(DeepLinkResult::Manga { key }));
			}
		}

		Ok(None)
	}
}

impl DynamicFilters for Se8 {
	fn get_dynamic_filters(&self) -> Result<Vec<Filter>> {
		let tags = Url::TagsPage.request()?.html()?.tags_filter()?.into();
		Ok([tags].into())
	}
}

register_source!(Se8, ImageRequestProvider, DeepLinkHandler, DynamicFilters);
