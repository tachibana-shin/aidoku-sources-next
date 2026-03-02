use crate::{BASE_URL, USER_AGENT, html::TagsPage as _};
use aidoku::{
	FilterValue, Result,
	alloc::{String, string::ToString as _},
	helpers::uri::encode_uri,
	imports::net::Request,
	prelude::{error, format},
};
use core::fmt::{Display, Formatter, Result as FmtResult};

const FILTER_ORDER: &[&str] = &["hits", "addtime"];

#[derive(Clone)]
pub enum Url {
	Search {
		query: String,
		page: i32,
	},
	Filter {
		tag: String,
		finish: String,
		order: String,
		page: i32,
	},
	TagsPage,
}

impl Url {
	pub fn from_query_or_filters(
		query: Option<&str>,
		page: i32,
		filters: &[FilterValue],
	) -> Result<Self> {
		if let Some(q) = query {
			return Ok(Self::Search {
				query: encode_uri(q),
				page,
			});
		}

		let mut tag = String::new();
		let mut finish = String::new();
		let mut order = String::from("hits");

		for filter in filters {
			match filter {
				FilterValue::Text { value, .. } => {
					return Ok(Self::Search {
						query: encode_uri(value),
						page,
					});
				}
				FilterValue::Select { id, value } => match id.as_str() {
					"标签" => tag = value.to_string(),
					"进度" => finish = value.to_string(),
					"genre" => {
						let tags = Self::TagsPage.request()?.html()?.tags_filter()?;
						let tag_id = tags
							.options
							.iter()
							.position(|option| option == value)
							.and_then(|index| tags.ids?.into_iter().nth(index))
							.ok_or_else(|| error!("Tag ID not found for option: `{value}`"))?;
						tag = tag_id.into();
					}
					_ => continue,
				},
				FilterValue::Sort {
					id,
					index,
					ascending: _,
				} => {
					if id == "排序"
						&& let Some(o) = FILTER_ORDER.get(*index as usize)
					{
						order = o.to_string();
					}
				}
				_ => continue,
			}
		}

		Ok(Self::Filter {
			tag,
			finish,
			order,
			page,
		})
	}

	pub fn request(&self) -> Result<Request> {
		let url = self.to_string();
		Ok(Request::get(url)?.header("User-Agent", USER_AGENT))
	}
}

impl Display for Url {
	fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
		match self {
			Url::Search { query, page } => {
				write!(f, "{}/search/{}/{}", BASE_URL, query, page)
			}
			Url::Filter {
				tag,
				finish,
				order,
				page,
			} => {
				let mut url = format!("{}/category", BASE_URL);
				if !tag.is_empty() {
					url.push_str(&format!("/tags/{}", tag));
				}
				if !finish.is_empty() {
					url.push_str(&format!("/finish/{}", finish));
				}
				url.push_str(&format!("/order/{}/page/{}", order, page));
				write!(f, "{}", url)
			}
			Url::TagsPage => {
				write!(f, "{}/category", BASE_URL)
			}
		}
	}
}
