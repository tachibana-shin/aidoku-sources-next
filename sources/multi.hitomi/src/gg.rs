use crate::{CDN_DOMAIN, LTN_URL, REFERER, models::HitomiGallery};
use aidoku::{
	alloc::{String, Vec},
	imports::net::Request,
	prelude::*,
};

#[derive(Clone)]
pub struct GgState {
	pub b: String,
	pub switch_cases: Vec<u32>,
	pub switch_o: u32,
	pub default_o: u32,
}

/// Fetch a new GgState.
pub fn get_new_gg() -> Option<GgState> {
	let url = format!("{LTN_URL}/gg.js");
	let body = Request::get(&url)
		.ok()?
		.header("Referer", REFERER)
		.string()
		.ok()?;
	parse_gg_js(&body)
}

fn parse_gg_js(body: &str) -> Option<GgState> {
	let b_start = body.find("b: '")?;
	let after_b = &body[b_start + 4..];
	let b_end = after_b.find('\'')?;
	let b: String = after_b[..b_end].into();

	let default_o = body
		.find("var o = ")
		.and_then(|i| {
			let s = &body[i + 8..];
			let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
			s[..end].parse::<u32>().ok()
		})
		.unwrap_or(0);

	let switch_o = {
		let mut found = None;
		let mut haystack = body;
		if let Some(skip) = haystack.find("var o = ") {
			haystack = &haystack[skip + 8..];
		}
		while let Some(pos) = haystack.find("o = ") {
			haystack = &haystack[pos + 4..];
			let end = haystack
				.find(|c: char| !c.is_ascii_digit())
				.unwrap_or(haystack.len());
			let rest = haystack[end..].trim_start();
			if rest.starts_with("; break") {
				found = haystack[..end].parse::<u32>().ok();
				break;
			}
		}
		found.unwrap_or(1)
	};

	let mut switch_cases: Vec<u32> = Vec::new();
	let mut search = body;
	while let Some(pos) = search.find("case ") {
		search = &search[pos + 5..];
		let end = search.find(':').unwrap_or(search.len());
		if let Ok(n) = search[..end].trim().parse::<u32>() {
			switch_cases.push(n);
		}
	}

	Some(GgState {
		b,
		switch_cases,
		switch_o,
		default_o,
	})
}

impl GgState {
	pub fn subdomain_offset(&self, image_id: u32) -> u32 {
		if self.switch_cases.contains(&image_id) {
			self.switch_o
		} else {
			self.default_o
		}
	}
}

fn image_id_from_hash(hash: &str) -> Option<u32> {
	if hash.len() < 3 {
		return None;
	}
	let len = hash.len();
	let g1 = &hash[len - 3..len - 1];
	let g2 = &hash[len - 1..len];
	let combined = format!("{g2}{g1}");
	u32::from_str_radix(&combined, 16).ok()
}

pub fn image_url(hash: &str, ext: &str, gg: &GgState) -> String {
	let image_id = match image_id_from_hash(hash) {
		Some(id) => id,
		None => return String::new(),
	};
	let offset = gg.subdomain_offset(image_id);
	let b = gg.b.trim_end_matches('/');
	format!(
		"https://a{}.{CDN_DOMAIN}/{b}/{image_id}/{hash}.{ext}",
		offset + 1,
	)
}

pub fn parse_galleryinfo_js(body: String) -> Option<HitomiGallery> {
	let needle = "galleryinfo = ";
	let start = body.find(needle)? + needle.len();
	let json = body[start..].trim_end_matches([';', '\n', '\r']);
	serde_json::from_str::<HitomiGallery>(json.trim()).ok()
}

pub fn fetch_gallery(id: i64) -> Option<HitomiGallery> {
	let url = format!("{LTN_URL}/galleries/{id}.js");
	let body = Request::get(&url)
		.ok()?
		.header("Referer", REFERER)
		.string()
		.ok()?;
	parse_galleryinfo_js(body)
}
