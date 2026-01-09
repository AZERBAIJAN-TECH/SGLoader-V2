pub(crate) fn display_tag(tag: &str) -> Option<String> {
    let lower = tag.to_lowercase();

    if let Some(code) = lower.strip_prefix("lang:") {
        return match code {
            "ru" => Some("русский".to_string()),
            "en" => Some("english".to_string()),
            _ => Some(format!("lang:{code}")),
        };
    }

    if let Some(code) = lower.strip_prefix("rp:") {
        return match code {
            "low" => Some("LRP".to_string()),
            "med" | "medium" => Some("MRP".to_string()),
            "high" => Some("HRP".to_string()),
            other => Some(format!("RP {}", other.to_uppercase())),
        };
    }

    if lower.starts_with("region:") {
        return None;
    }

    if lower == "tts" {
        return Some("TTS".to_string());
    }

    Some(tag.to_string())
}

pub(crate) fn display_region(region: &str) -> String {
    match region.to_lowercase().as_str() {
        "ru" | "russia" => "RU".to_string(),
        "eu" => "EU".to_string(),
        "eu-west" | "eu_west" | "eu-w" | "eu_w" => "EU-West".to_string(),
        "eu-east" | "eu_east" | "eu-e" | "eu_e" => "EU-East".to_string(),
        "na" => "NA".to_string(),
        "na-west" | "na_west" | "us-west" | "us_west" | "am_n_w" => "NA-West".to_string(),
        "na-east" | "na_east" | "us-east" | "us_east" | "am_n_e" => "NA-East".to_string(),
        "am_c" => "NA-Central".to_string(),
        "sa" => "SA".to_string(),
        "am_s" => "SA".to_string(),
        "asia" => "Asia".to_string(),
        "oce" | "oceania" => "Oceania".to_string(),
        "au" => "AU".to_string(),
        other => other.to_uppercase(),
    }
}

pub(crate) fn truncate_name(name: &str, limit: usize) -> String {
    let mut result = String::new();
    for (count, ch) in name.chars().enumerate() {
        if count >= limit {
            result.push_str("...");
            break;
        }
        result.push(ch);
    }
    result
}
