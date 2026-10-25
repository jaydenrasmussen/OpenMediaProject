//! Arabic <-> Roman numeral variants of a title, so "Rocky II" finds "Rocky 2".
//! Radarr maps 1 to 21.

/// Upper-case Roman numeral for 1..=3999.
pub fn to_roman(mut n: u32) -> String {
    const TABLE: [(u32, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut out = String::new();
    for (value, text) in TABLE {
        while n >= value {
            out.push_str(text);
            n -= value;
        }
    }
    out
}

/// (arabic as text, roman upper, roman lower) for 1..=21, in order.
pub static ARABIC_ROMAN: std::sync::LazyLock<Vec<(String, String, String)>> =
    std::sync::LazyLock::new(|| {
        (1..=21)
            .map(|n| {
                let roman = to_roman(n);
                (n.to_string(), roman.clone(), roman.to_lowercase())
            })
            .collect()
    });

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numerals() {
        let got: Vec<_> = [1, 2, 3, 4, 5, 9, 14, 19, 20, 21, 1994]
            .map(to_roman)
            .to_vec();
        assert_eq!(
            got,
            [
                "I", "II", "III", "IV", "V", "IX", "XIV", "XIX", "XX", "XXI", "MCMXCIV"
            ]
        );
    }

    #[test]
    fn mapping_covers_one_to_twenty_one() {
        assert_eq!(ARABIC_ROMAN.len(), 21);
        assert_eq!(ARABIC_ROMAN[3], ("4".into(), "IV".into(), "iv".into()));
    }
}
