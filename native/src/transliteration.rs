//! Display-only Devanagari romanization. The stored transcript stays untouched.
//! Long vowels and retroflex consonants retain their distinguishing marks.
pub(crate) fn contains_devanagari(text: &str) -> bool {
    text.chars().any(|c| ('\u{0900}'..='\u{097f}').contains(&c))
}

pub(crate) fn latin(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut word = String::new();
    for c in text.chars() {
        if ('\u{0900}'..='\u{097f}').contains(&c) && !matches!(c, '।' | '॥') {
            word.push(c);
        } else {
            out.push_str(&romanize(&word));
            word.clear();
            out.push(if matches!(c, '।' | '॥') { '.' } else { c });
        }
    }
    out.push_str(&romanize(&word));
    out
}

fn romanize(word: &str) -> String {
    let mut word = word.to_owned();
    for (base, precomposed) in [
        ("क़", "क़"),
        ("ख़", "ख़"),
        ("ग़", "ग़"),
        ("ज़", "ज़"),
        ("ड़", "ड़"),
        ("ढ़", "ढ़"),
        ("फ़", "फ़"),
        ("य़", "य़"),
    ] {
        word = word.replace(base, precomposed);
    }
    let mut out = String::new();
    let mut inherent = false;
    let mut consonants = 0;
    for c in word.chars() {
        let consonant = match c {
            'क' => "k",
            'ख' => "kh",
            'ग' => "g",
            'घ' => "gh",
            'ङ' => "ṅ",
            'च' => "ch",
            'छ' => "chh",
            'ज' => "j",
            'झ' => "jh",
            'ञ' => "ñ",
            'ट' => "ṭ",
            'ठ' => "ṭh",
            'ड' => "ḍ",
            'ढ' => "ḍh",
            'ण' => "ṇ",
            'त' => "t",
            'थ' => "th",
            'द' => "d",
            'ध' => "dh",
            'न' => "n",
            'प' => "p",
            'फ' => "ph",
            'ब' => "b",
            'भ' => "bh",
            'म' => "m",
            'य' => "y",
            'र' => "r",
            'ल' => "l",
            'व' => "v",
            'श' => "sh",
            'ष' => "ṣ",
            'स' => "s",
            'ह' => "h",
            'ळ' => "ḷ",
            'क़' => "q",
            'ख़' => "kh",
            'ग़' => "gh",
            'ज़' => "z",
            'ड़' => "ṛ",
            'ढ़' => "ṛh",
            'फ़' => "f",
            'य़' => "y",
            _ => "",
        };
        if !consonant.is_empty() {
            out.push_str(consonant);
            out.push('a');
            inherent = true;
            consonants += 1;
            continue;
        }
        let vowel = match c {
            'ा' => Some("ā"),
            'ि' => Some("i"),
            'ी' => Some("ī"),
            'ु' => Some("u"),
            'ू' => Some("ū"),
            'ृ' => Some("ṛ"),
            'ॄ' => Some("ṝ"),
            'े' => Some("e"),
            'ै' => Some("ai"),
            'ो' => Some("o"),
            'ौ' => Some("au"),
            'ॅ' => Some("e"),
            'ॉ' => Some("o"),
            '्' => Some(""),
            _ => None,
        };
        if let Some(v) = vowel {
            if inherent {
                out.pop();
            }
            out.push_str(v);
            inherent = false;
            continue;
        }
        match c {
            'अ' => out.push('a'),
            'आ' => out.push('ā'),
            'इ' => out.push('i'),
            'ई' => out.push('ī'),
            'उ' => out.push('u'),
            'ऊ' => out.push('ū'),
            'ए' => out.push('e'),
            'ऐ' => out.push_str("ai"),
            'ओ' => out.push('o'),
            'औ' => out.push_str("au"),
            'ऋ' => out.push('ṛ'),
            'ं' | 'ँ' => out.push('\u{0303}'),
            'ः' => out.push('ḥ'),
            'ऽ' => out.push('\''),
            'ॐ' => out.push_str("om"),
            '०'..='९' => out.push(char::from_u32('0' as u32 + c as u32 - '०' as u32).unwrap()),
            _ => out.push(c),
        }
        inherent = false;
    }
    // Hindi normally drops an unwritten vowel at the end of a multi-letter word.
    if inherent && consonants > 1 {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_english_is_unchanged() {
        assert_eq!(latin("Friday 18:30 — café"), "Friday 18:30 — café");
    }
    #[test]
    fn mixed_script_preserves_words_and_numbers() {
        assert_eq!(
            latin("तीन पुराने worker, अभी भी ३"),
            "tīn purāne worker, abhī bhī 3"
        );
        assert!(contains_devanagari("हिंदी"));
        assert!(!contains_devanagari("Hindi"));
    }
    #[test]
    fn no_schwa_after_explicit_halant_or_vowel() {
        assert_eq!(latin("भारत हिंदी ठीक।"), "bhārat hĩdī ṭhīk.");
    }
}
