//! `Plural-Forms` handles which langs have plurals

const ONE_FORM: &str = "nplurals=1; plural=0;";
const NOT_ONE: &str = "nplurals=2; plural=(n != 1);";
const ABOVE_ONE: &str = "nplurals=2; plural=(n > 1);";
const EAST_SLAVIC: &str = "nplurals=3; plural=(n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);";

pub(super) fn for_language(lang: &str) -> &'static str {
    lookup(lang)
        .or_else(|| lookup(lang.split(['_', '-']).next()?))
        .unwrap_or(NOT_ONE)
}

fn lookup(lang: &str) -> Option<&'static str> {
    Some(match lang {
        "ja" | "ko" | "zh" | "zh_CN" | "zh_TW" | "vi" | "th" | "id" | "ms" | "lo" | "jbo" => ONE_FORM,
        "fr" | "pt_BR" | "oc" | "fil" => ABOVE_ONE,
        "ru" | "uk" | "be" | "sr" | "hr" | "bs" => EAST_SLAVIC,
        "pl" => "nplurals=3; plural=(n==1 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);",
        "cs" | "sk" => "nplurals=3; plural=(n==1 ? 0 : n>=2 && n<=4 ? 1 : 2);",
        "lt" => "nplurals=3; plural=(n%10==1 && n%100!=11 ? 0 : n%10>=2 && (n%100<10 || n%100>=20) ? 1 : 2);",
        "lv" => "nplurals=3; plural=(n%10==1 && n%100!=11 ? 0 : n != 0 ? 1 : 2);",
        "ro" => "nplurals=3; plural=(n==1 ? 0 : (n==0 || (n%100 > 0 && n%100 < 20)) ? 1 : 2);",
        "sl" => "nplurals=4; plural=(n%100==1 ? 0 : n%100==2 ? 1 : n%100==3 || n%100==4 ? 2 : 3);",
        "ga" => "nplurals=5; plural=(n==1 ? 0 : n==2 ? 1 : n<7 ? 2 : n<11 ? 3 : 4);",
        "ar" => "nplurals=6; plural=(n==0 ? 0 : n==1 ? 1 : n==2 ? 2 : n%100>=3 && n%100<=10 ? 3 : n%100>=11 ? 4 : 5);",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match() {
        assert_eq!(for_language("fr"), ABOVE_ONE);
        assert_eq!(for_language("ja"), ONE_FORM);
    }

    #[test]
    fn region_specific_beats_base_language() {
        assert_eq!(for_language("pt_BR"), ABOVE_ONE);
        assert_eq!(for_language("pt"), NOT_ONE);
    }

    #[test]
    fn falls_back_to_base_language() {
        assert_eq!(for_language("sr_Cyrl"), EAST_SLAVIC);
        assert_eq!(for_language("fr_CA"), ABOVE_ONE);
    }

    #[test]
    fn unknown_falls_back_to_not_one() {
        assert_eq!(for_language("de"), NOT_ONE);
        assert_eq!(for_language("xx"), NOT_ONE);
    }
}
