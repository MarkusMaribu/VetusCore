/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Shared language matching for bundle metadata and resource lookup.

fn canonical(identifier: &str) -> String {
    let tag = identifier.replace('_', "-").to_ascii_lowercase();
    let tag = match tag.as_str() {
        "english" => "en",
        "spanish" => "es",
        "chinese" => "zh",
        "hindi" => "hi",
        "japanese" => "ja",
        "french" => "fr",
        "german" => "de",
        "italian" => "it",
        "danish" => "da",
        "dutch" => "nl",
        "finnish" => "fi",
        "korean" => "ko",
        "norwegian" => "no",
        "portuguese" => "pt",
        "russian" => "ru",
        "swedish" => "sv",
        "turkish" => "tr",
        _ => &tag,
    };
    // Old iOS bundles commonly use regional Chinese tags instead of scripts.
    match tag {
        "zh-cn" | "zh-sg" => "zh-hans".to_string(),
        "zh-tw" | "zh-hk" | "zh-mo" => "zh-hant".to_string(),
        _ => tag.to_string(),
    }
}

/// Return the best supported language, with its generic localization if any.
/// Rank by user preference, never by filesystem/HashMap enumeration order.
/// Apple's contract and the upstream implementation checked for this fix:
/// https://developer.apple.com/documentation/foundation/bundle/preferredlocalizations(from:)
/// https://github.com/touchHLE/touchHLE/blob/trunk/src/frameworks/core_foundation/cf_bundle.rs
pub(crate) fn preferred(available: &[String], preferences: &[String]) -> Vec<usize> {
    let tags: Vec<String> = available.iter().map(|s| canonical(s)).collect();
    for preference in preferences {
        let wanted = canonical(preference);
        let language = wanted.split('-').next().unwrap_or("");
        let mut matches: Vec<usize> = tags
            .iter()
            .enumerate()
            .filter_map(|(i, tag)| {
                if tag == "base" || tag.split('-').next() != Some(language) {
                    return None;
                }
                // Do not substitute the other Chinese script for an explicit one.
                if (wanted.contains("hans") && tag.contains("hant"))
                    || (wanted.contains("hant") && tag.contains("hans"))
                {
                    return None;
                }
                Some(i)
            })
            .collect();
        matches.sort_by_key(|&i| {
            (
                if tags[i] == wanted {
                    0
                } else if tags[i] == language {
                    1
                } else {
                    2
                },
                tags[i].clone(),
                available[i].clone(),
            )
        });
        if let Some(&best) = matches.first() {
            let mut result = vec![best];
            if tags[best] != language {
                if let Some(generic) = tags.iter().position(|tag| tag == language) {
                    result.push(generic);
                }
            }
            return result;
        }
    }
    Vec::new()
}

/// A generic recursive asset search must never choose a translation.
pub(super) fn is_localization_directory(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".lproj")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }
    fn choose(available: &[&str], preferences: &[&str]) -> Vec<String> {
        let available = strings(available);
        preferred(&available, &strings(preferences))
            .iter()
            .map(|&i| available[i].clone())
            .collect()
    }
    #[test]
    fn language_is_independent_of_bundle_enumeration_order() {
        for available in [
            ["fr", "zh-Hans", "en"],
            ["zh-Hans", "en", "fr"],
            ["en", "fr", "zh-Hans"],
        ] {
            assert_eq!(choose(&available, &["en"]), ["en"]);
            assert_eq!(choose(&available, &["zh", "en"]), ["zh-Hans"]);
            assert_eq!(choose(&available, &["hi", "en"]), ["en"]);
        }
        assert_eq!(choose(&["en", "es"], &["es", "en"]), ["es"]);
    }
    #[test]
    fn legacy_names_and_regional_tags_match() {
        assert_eq!(
            choose(&["French", "English", "Spanish"], &["es-MX", "en"]),
            ["Spanish"]
        );
        assert_eq!(choose(&["en", "en_GB"], &["en-GB"]), ["en_GB", "en"]);
        assert_eq!(choose(&["zh-Hant", "zh-Hans"], &["zh_CN"]), ["zh-Hans"]);
        assert_eq!(choose(&["zh-Hans", "en"], &["zh-TW", "en"]), ["en"]);
        assert_eq!(choose(&["hi", "ja"], &["ja", "hi"]), ["ja"]);
    }
    #[test]
    fn recursive_scan_excludes_every_localization_including_base() {
        assert!(is_localization_directory("fr.lproj"));
        assert!(is_localization_directory("Base.lproj"));
        assert!(!is_localization_directory("Sounds"));
    }
}
