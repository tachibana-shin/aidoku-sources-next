use aidoku::{alloc::String, imports::defaults::defaults_get};

const LANGUAGE_KEY: &str = "language";
const TITLE_PREFERENCE_KEY: &str = "titlePreference";

pub fn get_nozomi_language() -> String {
	defaults_get::<String>(LANGUAGE_KEY)
		.map(|lang| match lang.as_str() {
			"en" => "english".into(),
			"id" => "indonesian".into(),
			"jv" => "javanese".into(),
			"ca" => "catalan".into(),
			"ceb" => "cebuano".into(),
			"cs" => "czech".into(),
			"da" => "danish".into(),
			"de" => "german".into(),
			"et" => "estonian".into(),
			"es" => "spanish".into(),
			"eo" => "esperanto".into(),
			"fr" => "french".into(),
			"it" => "italian".into(),
			"hi" => "hindi".into(),
			"hu" => "hungarian".into(),
			"pl" => "polish".into(),
			"pt" => "portuguese".into(),
			"vi" => "vietnamese".into(),
			"tr" => "turkish".into(),
			"ru" => "russian".into(),
			"uk" => "ukrainian".into(),
			"ar" => "arabic".into(),
			"ko" => "korean".into(),
			"zh" => "chinese".into(),
			"ja" => "japanese".into(),
			_ => "all".into(),
		})
		.unwrap_or_else(|| "all".into())
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum TitlePreference {
	#[default]
	English,
	Japanese,
}

impl From<String> for TitlePreference {
	fn from(value: String) -> Self {
		match value.as_str() {
			"japanese" => Self::Japanese,
			_ => Self::English,
		}
	}
}

pub fn get_title_preference() -> TitlePreference {
	defaults_get::<String>(TITLE_PREFERENCE_KEY)
		.map(TitlePreference::from)
		.unwrap_or_default()
}
