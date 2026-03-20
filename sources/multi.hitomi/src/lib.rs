#![no_std]
use aidoku::{
	Chapter, DeepLinkHandler, DeepLinkResult, FilterValue, HashMap, ImageRequestProvider, Listing,
	ListingProvider, Manga, MangaPageResult, Page, PageContent, PageContext, Result, Source,
	alloc::{String, Vec, string::ToString, vec},
	imports::{
		net::Request,
		std::{current_date, parse_date},
	},
	prelude::*,
};
use core::cell::RefCell;

mod gg;
mod models;
mod search;
mod settings;

use gg::*;
use search::*;

use crate::gg::get_new_gg;

pub const BASE_URL: &str = "https://hitomi.la";
pub const REFERER: &str = "https://hitomi.la/";
pub const LTN_URL: &str = "https://ltn.gold-usergeneratedcontent.net";
pub const CDN_DOMAIN: &str = "gold-usergeneratedcontent.net";
pub const PAGE_SIZE: i32 = 25;

struct Hitomi {
	gg_cache: RefCell<Option<(GgState, i64)>>,
}

impl Source for Hitomi {
	fn new() -> Self {
		Self {
			gg_cache: RefCell::new(None),
		}
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		let lang = settings::get_nozomi_language();

		let mut sort_area: Option<String> = None;
		let mut sort_tag: String = "index".into();
		let mut type_filter: Option<String> = None;

		let mut artist_filter: Option<String> = None;
		let mut group_filter: Option<String> = None;
		let mut author_name: Option<String> = None;
		let mut genre_filter: Option<String> = None;

		for f in filters {
			match f {
				FilterValue::Sort { index, .. } => match index {
					1 => {
						sort_area = Some("popular".into());
						sort_tag = "published".into();
					}
					2 => {
						sort_area = Some("popular".into());
						sort_tag = "today".into();
					}
					3 => {
						sort_area = Some("popular".into());
						sort_tag = "week".into();
					}
					4 => {
						sort_area = Some("popular".into());
						sort_tag = "month".into();
					}
					5 => {
						sort_area = Some("popular".into());
						sort_tag = "year".into();
					}
					_ => {
						sort_area = None;
						sort_tag = "index".into();
					}
				},
				FilterValue::Text { id, value } if !value.is_empty() => match id.as_str() {
					"author" => {
						author_name = Some(value.to_lowercase().replace(' ', "_"));
					}
					"artist" => {
						artist_filter =
							Some(format!("artist:{}", value.to_lowercase().replace(' ', "_")));
					}
					"group" => {
						group_filter =
							Some(format!("group:{}", value.to_lowercase().replace(' ', "_")));
					}
					_ => {}
				},
				FilterValue::Select { id, value } => match id.as_str() {
					"type" if !value.is_empty() => {
						type_filter = Some(value);
					}
					"genre" => {
						// '♀' -> female:, '♂' -> male:
						let mut v = value.to_lowercase();
						let female = v.contains('♀');
						let male = v.contains('♂');
						// remove gender symbols and normalize
						v = v.replace(['♀', '♂'], "").trim().replace(' ', "_");
						genre_filter = Some(if female {
							format!("female:{v}")
						} else if male {
							format!("male:{v}")
						} else {
							format!("tag:{v}")
						});
					}
					_ => {}
				},
				_ => {}
			}
		}

		let raw_q: String = query.as_deref().unwrap_or_default().trim().into();

		// numeric ID or URL shortcut — resolve to gallery ID and return directly
		let shortcut_id: Option<i64> = if raw_q.is_empty() {
			None
		} else if let Ok(id) = raw_q.parse::<i64>() {
			// bare numeric ID
			Some(id)
		} else if raw_q.contains("hitomi.la") {
			// full URL — extract ID using the same logic as DeepLinkHandler
			extract_hitomi_id(&raw_q)
		} else {
			None
		};
		if let Some(id) = shortcut_id
			&& let Some(g) = fetch_gallery(id)
		{
			return Ok(MangaPageResult {
				entries: vec![g.into()],
				has_next_page: false,
			});
		}

		let mut positive_terms: Vec<String> = Vec::new();
		let mut negative_terms: Vec<String> = Vec::new();

		{
			let tokens: Vec<&str> = raw_q.split_whitespace().collect();
			for tok in tokens {
				let tok = tok.to_lowercase();
				if let Some(neg) = tok.strip_prefix('-') {
					if !neg.is_empty() {
						negative_terms.push(neg.into());
					}
				} else {
					positive_terms.push(tok);
				}
			}
		}

		if let Some(t) = artist_filter {
			positive_terms.push(t);
		}
		if let Some(t) = group_filter {
			positive_terms.push(t);
		}
		if let Some(t) = genre_filter {
			positive_terms.push(t);
		}

		let is_default_sort = sort_area.is_none() && sort_tag == "index";
		if lang != "all"
			&& is_default_sort
			&& !positive_terms
				.iter()
				.chain(negative_terms.iter())
				.any(|t| t.starts_with("language:"))
		{
			positive_terms.push(format!("language:{lang}"));
		}

		let sort_nozomi_url = match &sort_area {
			Some(area) => format!("{LTN_URL}/{area}/{sort_tag}-{lang}.nozomi"),
			None => format!("{LTN_URL}/{sort_tag}-{lang}.nozomi"),
		};

		let use_sort_base =
			(positive_terms.is_empty() && author_name.is_none()) || !is_default_sort;

		// Parallelize network-bound term queries (ns:tag nozomi requests).
		let mut positive_results: Vec<Vec<i64>> = Vec::new();
		// Collect local (plain text) results immediately, and build nozomi requests for network ones
		let mut local_results: Vec<(usize, Vec<i64>)> = Vec::new();
		let mut nozomi_requests: Vec<(usize, String)> = Vec::new();
		for (i, term) in positive_terms.iter().enumerate() {
			if term.contains(':') {
				if let Some(url) = nozomi_url_for_ns_tag(term, &lang) {
					nozomi_requests.push((i, url));
				} else {
					bail!("Unsupported namespace for term: {term}");
				}
			} else if let Some(ids) = search_plain_text(term) {
				local_results.push((i, ids));
			} else {
				bail!("Search failed for term: {term}");
			}
		}

		// Insert local results into positive_results at the correct index
		positive_results.resize(positive_terms.len(), Vec::new());
		for (i, ids) in local_results {
			positive_results[i] = ids;
		}

		// Fire batch requests for nozomi URLs
		if !nozomi_requests.is_empty() {
			let mut reqs: Vec<Request> = Vec::new();
			for (_i, url) in &nozomi_requests {
				reqs.push(Request::get(url)?.header("Referer", REFERER));
			}
			let responses = Request::send_all(reqs);
			// responses length matches reqs; map back to indexes
			for (resp_i, resp) in responses.into_iter().enumerate() {
				let idx = nozomi_requests[resp_i].0;
				match resp {
					Ok(r) => match r.get_data() {
						Ok(data) => positive_results[idx] = decode_nozomi(&data),
						Err(_) => positive_results[idx] = Vec::new(),
					},
					Err(_) => positive_results[idx] = Vec::new(),
				}
			}
		}

		// "author" from detail page: union of artist: and group: nozomis (parallel)
		if let Some(ref name) = author_name {
			let artist_term = format!("artist:{name}");
			let group_term = format!("group:{name}");
			let mut reqs: Vec<Request> = Vec::new();
			let mut want_artist = false;
			let mut want_group = false;
			if let Some(url) = nozomi_url_for_ns_tag(&artist_term, &lang) {
				reqs.push(Request::get(&url)?.header("Referer", REFERER));
				want_artist = true;
			}
			if let Some(url) = nozomi_url_for_ns_tag(&group_term, &lang) {
				reqs.push(Request::get(&url)?.header("Referer", REFERER));
				want_group = true;
			}

			let mut artist_vec: Option<Vec<i64>> = None;
			let mut group_vec: Option<Vec<i64>> = None;
			if !reqs.is_empty() {
				let responses = Request::send_all(reqs);
				let mut resp_iter = responses.into_iter().flatten();
				if want_artist
					&& let Some(r) = resp_iter.next()
					&& let Ok(data) = r.get_data()
				{
					artist_vec = Some(decode_nozomi(&data));
				}
				if want_group
					&& let Some(r) = resp_iter.next()
					&& let Ok(data) = r.get_data()
				{
					group_vec = Some(decode_nozomi(&data));
				}
			}

			let mut union: Vec<i64> = match (artist_vec, group_vec) {
				(Some(a), Some(b)) => {
					let mut v = a;
					v.extend(b);
					v
				}
				(Some(a), None) => a,
				(None, Some(b)) => b,
				(None, None) => return Err(error!("No results for author: {name}")),
			};
			union.sort_unstable();
			union.dedup();
			positive_results.push(union);
		}

		let mut negative_ids: Vec<i64> = Vec::new();
		// Parallelize negative_terms requests: local plain-text first, batch nozomi for ns:tags
		if !negative_terms.is_empty() {
			let mut neg_local: Vec<Vec<i64>> = Vec::new();
			let mut neg_reqs: Vec<String> = Vec::new();
			for term in &negative_terms {
				if term.contains(':') {
					if let Some(url) = nozomi_url_for_ns_tag(term, &lang) {
						neg_reqs.push(url);
					}
				} else if let Some(ids) = search_plain_text(term) {
					neg_local.push(ids);
				}
			}
			for v in neg_local {
				negative_ids.extend(v);
			}
			if !neg_reqs.is_empty() {
				let mut reqs: Vec<Request> = Vec::new();
				for url in &neg_reqs {
					reqs.push(Request::get(url)?.header("Referer", REFERER));
				}
				let responses = Request::send_all(reqs);
				for r in responses.into_iter().flatten() {
					if let Ok(data) = r.get_data() {
						negative_ids.extend(decode_nozomi(&data));
					}
				}
			}
		}
		negative_ids.sort_unstable();
		negative_ids.dedup();

		let mut result_ids: Vec<i64> = if use_sort_base {
			let data = Request::get(&sort_nozomi_url)?
				.header("Referer", REFERER)
				.data()?;
			decode_nozomi(&data)
		} else {
			Vec::new()
		};

		for pos_ids in positive_results {
			if result_ids.is_empty() {
				result_ids = pos_ids;
			} else {
				let pos_set: Vec<i64> = {
					let mut v = pos_ids;
					v.sort_unstable();
					v
				};
				result_ids.retain(|id| pos_set.binary_search(id).is_ok());
			}
		}

		if let Some(ref t) = type_filter {
			let type_url = format!("{LTN_URL}/type/{t}-{lang}.nozomi");
			if let Ok(data) =
				Request::get(&type_url).and_then(|r| r.header("Referer", REFERER).data())
			{
				let mut type_ids = decode_nozomi(&data);
				type_ids.sort_unstable();
				result_ids.retain(|id| type_ids.binary_search(id).is_ok());
			}
		}

		if !negative_ids.is_empty() {
			result_ids.retain(|id| negative_ids.binary_search(id).is_err());
		}

		let start = ((page - 1) * PAGE_SIZE) as usize;
		let end = (start + PAGE_SIZE as usize).min(result_ids.len());
		let has_next_page = end < result_ids.len();
		let page_ids = if start < result_ids.len() {
			&result_ids[start..end]
		} else {
			&[]
		};

		// Parallelize fetching galleries for the current page via batch requests.
		let mut entries: Vec<Manga> = Vec::new();
		if !page_ids.is_empty() {
			let mut gallery_reqs: Vec<Request> = Vec::new();
			for id in page_ids {
				let url = format!("{LTN_URL}/galleries/{id}.js");
				gallery_reqs.push(Request::get(&url)?.header("Referer", REFERER));
			}

			// send_all returns Vec<Result<Response, RequestError>>
			let responses = Request::send_all(gallery_reqs);
			for resp in responses.into_iter() {
				if let Ok(r) = resp
					&& let Ok(body) = r.get_string()
					&& let Some(g) = parse_galleryinfo_js(body)
				{
					entries.push(g.into());
				}
			}
		}

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
		let id: i64 = manga
			.key
			.parse()
			.map_err(|_| error!("Invalid gallery id"))?;
		let gallery = fetch_gallery(id).ok_or_else(|| error!("Failed to fetch gallery"))?;
		let chapters = if needs_chapters {
			let scanlators = gallery
				.language
				.as_ref()
				.filter(|l| !l.is_empty())
				.map(|l| vec![l.clone()]);
			let date_uploaded =
				parse_date(&gallery.date[..10.min(gallery.date.len())], "yyyy-MM-dd");
			let chapter = Chapter {
				key: manga.key.clone(),
				chapter_number: Some(1.0),
				date_uploaded,
				url: Some(format!("{BASE_URL}/reader/{id}.html")),
				scanlators,
				..Default::default()
			};
			Some(vec![chapter])
		} else {
			None
		};
		if needs_details {
			let new_manga: Manga = gallery.into();
			manga.copy_from(new_manga);
		}
		if needs_chapters {
			manga.chapters = chapters;
		}
		Ok(manga)
	}

	fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let id: i64 = chapter
			.key
			.parse()
			.map_err(|_| error!("Invalid gallery id"))?;
		let gallery = fetch_gallery(id).ok_or_else(|| error!("Failed to fetch gallery"))?;

		// Fetch gg.js to get current subdomain routing state
		{
			let now = current_date();
			let needs_update = {
				let cached = self.gg_cache.borrow();
				cached.as_ref().is_none_or(|&(_, ts)| now - ts > 60)
			};
			if needs_update && let Some(gg) = get_new_gg() {
				*self.gg_cache.borrow_mut() = Some((gg, now));
			}
		}
		let cache = self.gg_cache.borrow();
		let gg = cache
			.as_ref()
			.ok_or_else(|| error!("Failed to fetch gg.js"))?;

		let reader_url = format!("{BASE_URL}/reader/{id}.html");
		let pages = gallery
			.files
			.iter()
			.map(|f| {
				let ext = if f.is_gif() { "webp" } else { "avif" };
				let url = image_url(&f.hash, ext, &gg.0);
				let mut ctx: PageContext = HashMap::new();
				ctx.insert("referer".into(), reader_url.clone());
				Page {
					content: PageContent::url_context(url, ctx),
					..Default::default()
				}
			})
			.collect();
		Ok(pages)
	}
}

impl ListingProvider for Hitomi {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		let lang = settings::get_nozomi_language();
		let url = match listing.id.as_str() {
			"popular_today" => format!("{LTN_URL}/popular/today-{lang}.nozomi"),
			"popular_week" => format!("{LTN_URL}/popular/week-{lang}.nozomi"),
			"popular_month" => format!("{LTN_URL}/popular/month-{lang}.nozomi"),
			"popular_year" => format!("{LTN_URL}/popular/year-{lang}.nozomi"),
			_ => format!("{LTN_URL}/index-{lang}.nozomi"),
		};
		let (ids, has_next_page) = fetch_nozomi_page(&url, page)?;

		let entries: Vec<Manga> = ids
			.into_iter()
			.filter_map(fetch_gallery)
			.map(|g| g.into())
			.collect();

		Ok(MangaPageResult {
			entries,
			has_next_page,
		})
	}
}

/// Extract a gallery ID from any hitomi.la URL format:
/// - https://hitomi.la/reader/123456.html
/// - https://hitomi.la/g/123456/
/// - https://hitomi.la/galleries/manga-title-123456.html
fn extract_hitomi_id(url: &str) -> Option<i64> {
	if let Some(idx) = url.find("/reader/") {
		let rest = &url[idx + "/reader/".len()..];
		let end = rest.find('.').unwrap_or(rest.len());
		return rest[..end].parse().ok();
	}
	if let Some(idx) = url.find("/g/") {
		let rest = &url[idx + "/g/".len()..];
		let end = rest.find('/').unwrap_or(rest.len());
		return rest[..end].parse().ok();
	}
	if let Some(dot) = url.rfind(".html") {
		let path = &url[..dot];
		if let Some(dash) = path.rfind('-') {
			let maybe_id = &path[dash + 1..];
			if maybe_id.chars().all(|c| c.is_ascii_digit()) {
				return maybe_id.parse().ok();
			}
		}
	}
	None
}

impl DeepLinkHandler for Hitomi {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		if !url.contains("hitomi.la") {
			return Ok(None);
		}
		Ok(extract_hitomi_id(&url).map(|id| DeepLinkResult::Manga {
			key: id.to_string(),
		}))
	}
}

impl ImageRequestProvider for Hitomi {
	fn get_image_request(&self, url: String, context: Option<PageContext>) -> Result<Request> {
		let referer = context
			.as_ref()
			.and_then(|c| c.get("referer").map(|s| s.as_str()))
			.unwrap_or(REFERER);
		Ok(Request::get(url)?
			.header("Referer", referer)
			.header("Origin", BASE_URL)
			.header(
				"Accept",
				"image/webp,image/avif,image/apng,image/svg+xml,image/*,*/*;q=0.8",
			))
	}
}

register_source!(
	Hitomi,
	ListingProvider,
	ImageRequestProvider,
	DeepLinkHandler
);
