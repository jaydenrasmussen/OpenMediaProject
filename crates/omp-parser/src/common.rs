//! Patterns shared between parsers (Radarr `ParserCommon`, GPL-3.0).

use std::sync::LazyLock;

use crate::re::Re;

/// Leading "www.site.com - " or "[site.com] " prefixes.
pub static WEBSITE_PREFIX: LazyLock<Re> = LazyLock::new(|| {
    Re::ci(
        r"^(?:(?:\[|\()\s*)?(?:www\.)?[-a-z0-9-]{1,256}\.(?<!Naruto-Kun\.)(?:[a-z]{2,6}\.[a-z]{2,6}|xn--[a-z0-9-]{4,}|[a-z]{2,})\b(?:\s*(?:\]|\))|[ -]{2,})[ -]*",
    )
});

/// Trailing "[site.com]" tags.
pub static WEBSITE_POSTFIX: LazyLock<Re> = LazyLock::new(|| {
    Re::ci(r"(?:\[\s*)?(?:www\.)?[-a-z0-9-]{1,256}\.(?:xn--[a-z0-9-]{4,}|[a-z]{2,6})\b(?:\s*\])$")
});

/// Trailing "[ettv]"-style tracker tags.
pub static CLEAN_TORRENT_SUFFIX: LazyLock<Re> =
    LazyLock::new(|| Re::ci(r"\[(?:ettv|rartv|rarbg|cttv|publichd)\]$"));
