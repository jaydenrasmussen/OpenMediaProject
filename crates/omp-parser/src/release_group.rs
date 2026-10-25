//! Release group detection. Ported from Radarr `ReleaseGroupParser` (GPL-3.0).

use std::sync::LazyLock;

use crate::{
    common::{CLEAN_TORRENT_SUFFIX, WEBSITE_PREFIX},
    files::remove_file_extension,
    re::Re,
};

// .NET allowed `releasegroup` twice; the two alternatives are named rg1/rg2.
// Uses a back-reference inside a look-behind, so this one runs on `regress`.
static RELEASE_GROUP: LazyLock<Re> = LazyLock::new(|| {
    Re::ci(
        r"-(?<rg1>[a-z0-9]+(?<part2>-[a-z0-9]+)?(?!.+?(?:480p|576p|720p|1080p|2160p)))(?<!(?:WEB-(DL|Rip)|Blu-Ray|480p|576p|720p|1080p|2160p|DTS-HD|DTS-X|DTS-MA|DTS-ES|-ES|-EN|-CAT|-ENG|-JAP|-GER|-FRA|-FRE|-ITA|-HDRip|\d{1,2}-bit|[ ._]\d{4}-\d{2}|-\d{2}|tmdb(id)?-(?<tmdbid>\d+)|(?<imdbid>tt\d{7,8}))(?:\k<part2>)?)(?:\b|[-._ ]|$)|[-._ ]\[(?<rg2>[a-z0-9]+)\]$",
    )
});

static INVALID_RELEASE_GROUP: LazyLock<Re> = LazyLock::new(|| Re::ci(r"^([se]\d+|[0-9a-f]{8})$"));

static ANIME_RELEASE_GROUP: LazyLock<Re> =
    LazyLock::new(|| Re::ci(r"^(?:\[(?<subgroup>(?!\s).+?(?<!\s))\](?:_|-|\s|\.)?)"));

// Exception groups that do not follow "-RlsGrp". Manual list; be careful, it
// is a source of false positives.
static EXCEPTION_EXACT: LazyLock<Re> = LazyLock::new(|| {
    Re::ci(
        r"\b(?<releasegroup>KRaLiMaRKo|E\.N\.D|D\-Z0N3|Koten_Gars|BluDragon|ZØNEHD|HQMUX|VARYG|YIFY|YTS(.(MX|LT|AG))?|TMd|Eml HDTeam|LMain|DarQ|BEN THE MEN|TAoE|QxR|126811)\b",
    )
});

// Groups whose releases end with "RlsGroup)" or "RlsGroup]".
static EXCEPTION: LazyLock<Re> = LazyLock::new(|| {
    Re::ci(
        r"(?<=[._ \[])(?<releasegroup>(Silence|afm72|Panda|Ghost|MONOLITH|Tigole|Joy|ImE|UTR|t3nzin|Anime Time|Project Angel|Hakata Ramen|HONE|GiLG|Vyndros|SEV|Garshasp|Kappa|Natty|RCVR|SAMPA|YOGI|r00t|EDGE2020|RZeroX|FreetheFish|Anna|Bandi|Qman|theincognito|HDO|DusIctv|DHD|CtrlHD|-ZR-|ADC|XZVN|RH|Kametsu|Celdra)(?=\]|\)))",
    )
});

static CLEAN_RELEASE_GROUP: LazyLock<Re> = LazyLock::new(|| {
    Re::ci(
        r"(-(RP|1|NZBGeek|Obfuscated|Obfuscation|Scrambled|sample|Pre|postbot|xpost|Rakuv[a-z0-9]*|WhiteRev|BUYMORE|AsRequested|AlternativeToRequested|GEROV|Z0iDS3N|Chamele0n|4P|4Planet|AlteZachen|RePACKPOST))+$",
    )
});

/// Returns the release group, if the title names one.
pub fn parse_release_group(title: &str) -> Option<String> {
    let title = remove_file_extension(title.trim());
    let title = WEBSITE_PREFIX.replace_all(&title, "");
    let title = CLEAN_TORRENT_SUFFIX.replace_all(&title, "");

    if let Some(anime) = ANIME_RELEASE_GROUP.captures(&title) {
        return anime.name("subgroup").map(str::to_owned);
    }

    let title = CLEAN_RELEASE_GROUP.replace_all(&title, "");

    if let Some(last) = EXCEPTION_EXACT.captures_all(&title).pop() {
        return last.name("releasegroup").map(str::to_owned);
    }
    if let Some(last) = EXCEPTION.captures_all(&title).pop() {
        return last.name("releasegroup").map(str::to_owned);
    }

    let last = RELEASE_GROUP.captures_all(&title).pop()?;
    let group = last.name("rg1").or_else(|| last.name("rg2"))?;
    if group.parse::<i64>().is_ok() || INVALID_RELEASE_GROUP.is_match(group) {
        return None;
    }
    Some(group.to_owned())
}
