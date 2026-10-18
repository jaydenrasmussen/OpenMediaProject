//! Cases ported from Radarr's OrganizerTests fixtures (GPL-3.0).

use omp_naming::*;

fn movie() -> NamingMovie {
    NamingMovie {
        title: "South Park".into(),
        original_title: Some("South of the Park".into()),
        year: 2020,
        imdb_id: Some("tt1234567".into()),
        tmdb_id: 124578,
        certification: Some("R".into()),
        translations: vec![
            ("de".into(), "German South Park".into()),
            ("fr".into(), "French South Park".into()),
        ],
        ..Default::default()
    }
}

fn file() -> NamingFile {
    NamingFile {
        quality_title: Some("HDTV-720p".into()),
        revision_version: 1,
        release_group: Some("RadarrTest".into()),
        relative_path: Some("Movie Title (2020)/30 Rock - S01E01 - Test.mkv".into()),
        ..Default::default()
    }
}

fn config(format: &str) -> NamingConfig {
    NamingConfig {
        rename_movies: true,
        standard_movie_format: format.into(),
        ..Default::default()
    }
}

fn name(format: &str) -> String {
    build_file_name(&movie(), &file(), &config(format)).unwrap()
}

fn folder(m: &NamingMovie, format: &str) -> String {
    build_movie_folder(
        m,
        &NamingConfig {
            movie_folder_format: format.into(),
            ..Default::default()
        },
    )
}

fn with_title(title: &str) -> NamingMovie {
    NamingMovie {
        title: title.into(),
        ..Default::default()
    }
}

#[test]
fn title_tokens_follow_the_pattern_separator_and_case() {
    assert_eq!(name("{Movie Title}"), "South Park");
    assert_eq!(name("{Movie_Title}"), "South_Park");
    assert_eq!(name("{Movie.Title}"), "South.Park");
    assert_eq!(name("{Movie-Title}"), "South-Park");
    assert_eq!(name("{MOVIE TITLE}"), "SOUTH PARK");
    assert_eq!(name("{movie title}"), "south park");
    assert_eq!(name("{mOvIe-tItLE}"), "South-Park");
}

#[test]
fn translated_titles() {
    assert_eq!(name("{Movie Title:FR}"), "French South Park");
    assert_eq!(
        name("{Movie Title:JP}"),
        "South Park",
        "not a language code"
    );
    assert_eq!(name("{Movie Title:JA}"), "South Park", "no translation");
    assert_eq!(name("{Movie Title:JP|FR}"), "French South Park");
    assert_eq!(name("{Movie Title:JP|CN}"), "South Park");
}

#[test]
fn simple_movie_tokens() {
    assert_eq!(name("{Movie OriginalTitle}"), "South of the Park");
    assert_eq!(name("{Movie Certification}"), "R");
    assert_eq!(name("{Movie Collection}"), "");
    assert_eq!(
        name("{Movie CleanTitle} ({Release Year}) [{Quality Title}] [tmdb-{TmdbId}]"),
        "South Park (2020) [HDTV-720p] [tmdb-124578]"
    );
    assert_eq!(name("{Quality Title}"), "HDTV-720p");
    assert_eq!(name("{Release Group}"), "RadarrTest");
}

#[test]
fn clean_title_cases() {
    for (title, expected) in [
        ("Florence + the Machine", "Florence + the Machine"),
        ("Beyoncé X10", "Beyonce X10"),
        (
            "Girlfriends' Guide to Divorce",
            "Girlfriends Guide to Divorce",
        ),
        (
            "Rule #23: Never Lie to the Kids",
            "Rule #23 Never Lie to the Kids",
        ),
        (
            "Anne Hathaway/Florence + The Machine",
            "Anne Hathaway Florence + The Machine",
        ),
        ("Karma's a B*tch!", "Karmas a B-tch!"),
        (
            "Ke$ha: My Crazy Beautiful Life",
            "Ke$ha My Crazy Beautiful Life",
        ),
        ("$#*! My Dad Says", "$#-! My Dad Says"),
        ("Free! - Iwatobi Swim Club", "Free! Iwatobi Swim Club"),
        (
            "Marvel's Agents of S.H.I.E.L.D.",
            "Marvels Agents of S.H.I.E.L.D",
        ),
        ("Castle (2009)", "Castle 2009"),
        ("Law & Order (UK)", "Law and Order UK"),
        ("Is this okay?", "Is this okay"),
        ("[a] title", "a title"),
        ("backslash \\ backlash", "backslash backlash"),
        ("I'm the Boss", "Im the Boss"),
        ("That'll Be The Day", "Thatll Be The Day"),
        ("I Can't Die", "I Cant Die"),
        ("Won`t Get Fooled Again", "Wont Get Fooled Again"),
        ("Don’t Blink", "Dont Blink"),
        ("The ` Legend of Kings", "The Legend of Kings"),
        ("Joker: Folie à deux", "Joker Folie a deux"),
    ] {
        let m = with_title(title);
        let got = build_file_name(&m, &file(), &config("{Movie CleanTitle}")).unwrap();
        assert_eq!(got, expected, "{title}");
    }
}

#[test]
fn title_the_and_clean_title_the() {
    for (title, the, clean) in [
        ("The Mist", "Mist, The", "Mist, The"),
        (
            "A Place to Call Home",
            "Place to Call Home, A",
            "Place to Call Home, A",
        ),
        ("The Flash (2010)", "Flash, The (2010)", "Flash, The 2010"),
        (
            "The Fixer (ZH) (2015)",
            "Fixer, The (ZH) (2015)",
            "Fixer, The ZH 2015",
        ),
        (
            "The Rat Pack (A&E)",
            "Rat Pack, The (A&E)",
            "Rat Pack, The AandE",
        ),
        (
            "The Climax: I (Almost) Got Away With It (2016)",
            "Climax - I (Almost) Got Away With It, The (2016)",
            "Climax I Almost Got Away With It, The 2016",
        ),
        ("A", "A", "A"),
        ("Anne", "Anne", "Anne"),
        ("Theodore", "Theodore", "Theodore"),
        ("3%", "3%", "3%"),
    ] {
        let m = with_title(title);
        assert_eq!(
            build_file_name(&m, &file(), &config("{Movie TitleThe}")).unwrap(),
            the,
            "{title}"
        );
        assert_eq!(
            build_file_name(&m, &file(), &config("{Movie CleanTitleThe}")).unwrap(),
            clean,
            "{title}"
        );
    }
    assert_eq!(
        build_file_name(&with_title(""), &file(), &config("{Movie CleanTitleThe}")).unwrap(),
        ""
    );
}

#[test]
fn first_character_folders() {
    for (title, parent, child) in [
        ("The Mist", "M", "The Mist"),
        ("A", "A", "A"),
        ("30 Rock", "3", "30 Rock"),
        ("The '80s Greatest", "8", "The '80s Greatest"),
        ("좀비버스", "좀", "좀비버스"),
        ("¡Mucha Lucha!", "M", "¡Mucha Lucha!"),
        (".hack", "H", "hack"),
        ("Ütopya", "U", "Ütopya"),
        ("Æon Flux", "A", "Æon Flux"),
    ] {
        assert_eq!(
            folder(
                &with_title(title),
                "{Movie TitleFirstCharacter}\\{Movie Title}"
            ),
            format!("{parent}/{child}"),
            "{title}"
        );
    }
    assert_eq!(
        folder(
            &with_title("Westworld"),
            "{movie titlefirstcharacter}\\{movie title}"
        ),
        "w/westworld"
    );
}

#[test]
fn quality_proper_and_real() {
    let proper = NamingFile {
        revision_version: 2,
        ..file()
    };
    let real = NamingFile {
        revision_real: 1,
        ..file()
    };
    let n = |f: &NamingFile, p: &str| build_file_name(&movie(), f, &config(p)).unwrap();
    assert_eq!(n(&proper, "{Quality Proper}"), "Proper");
    assert_eq!(n(&real, "{Quality Real}"), "REAL");
    assert_eq!(name("{Quality Title} {Quality Proper}"), "HDTV-720p");
    assert_eq!(
        n(
            &proper,
            "{Movie Title} [{Quality Title}] {[Quality Proper]}"
        ),
        "South Park [HDTV-720p] [Proper]"
    );
    assert_eq!(
        name("{Movie Title} [{Quality Title}] {[Quality Proper]}"),
        "South Park [HDTV-720p]"
    );
    assert_eq!(
        name("{Movie Title} [{Quality Full}]"),
        "South Park [HDTV-720p]"
    );
    assert_eq!(
        n(&proper, "{Movie Title} [{Quality Full}]"),
        "South Park [HDTV-720p Proper]"
    );
    assert_eq!(
        n(&real, "{Movie Title} [{Quality Full}]"),
        "South Park [HDTV-720p REAL]"
    );
    for sep in [' ', '-', '.', '_'] {
        assert_eq!(
            name(&format!("{{Quality{sep}Title}}{sep}{{Quality{sep}Proper}}")),
            "HDTV-720p"
        );
        assert_eq!(
            name(&format!(
                "{{Quality{sep}Title}}{sep}{{Quality{sep}Proper}}{sep}{{Movie{sep}Title}}"
            )),
            format!("HDTV-720p{sep}South{sep}Park")
        );
    }
}

#[test]
fn affixes_and_separators() {
    assert_eq!(
        name("{Movie.Title}.{_Quality.Title_}"),
        "South.Park._HDTV-720p"
    );
    let n = |t: &str, p: &str| build_file_name(&with_title(t), &file(), &config(p)).unwrap();
    // Trailing separators are always trimmed (Radarr's test for keeping the
    // final period is commented out upstream).
    assert_eq!(n("Chicago P.D.", "{Movie.Title}."), "Chicago.P.D");
    assert_eq!(n("Venture Bros.", "{Movie.Title}"), "Venture.Bros");
}

#[test]
fn release_group_default_only_without_affixes() {
    let nogroup = NamingFile {
        release_group: None,
        ..file()
    };
    let n = |p: &str| build_file_name(&movie(), &nogroup, &config(p)).unwrap();
    assert_eq!(n("{Release Group}"), DEFAULT_RELEASE_GROUP);
    for p in [
        "{Movie Title}{-Release Group}",
        "{Movie Title}{ Release Group}",
        "{Movie Title}{ [Release Group]}",
    ] {
        assert_eq!(n(p), "South Park", "{p}");
    }
}

#[test]
fn original_title_and_filename() {
    let scene = NamingFile {
        scene_name: Some("30.Rock.S01E01.xvid-LOL".into()),
        ..file()
    };
    let n =
        |f: &NamingFile, p: &str| build_file_name(&with_title("30 Rock"), f, &config(p)).unwrap();
    assert_eq!(
        n(&scene, "{Movie Title} - {Original Title}"),
        "30 Rock - 30.Rock.S01E01.xvid-LOL"
    );
    assert_eq!(n(&scene, "{Original Title}"), "30.Rock.S01E01.xvid-LOL");
    assert_eq!(n(&file(), "{Original Title}"), "30 Rock - S01E01 - Test");
    assert_eq!(n(&file(), "{Original Filename}"), "30 Rock - S01E01 - Test");
    // With several tokens the original name is left out to avoid repeating it.
    assert_eq!(n(&file(), "{Movie Title} {Original Title}"), "30 Rock");

    let off = NamingConfig {
        rename_movies: false,
        ..Default::default()
    };
    assert_eq!(
        build_file_name(&movie(), &scene, &off).unwrap(),
        "30.Rock.S01E01.xvid-LOL"
    );
    assert_eq!(
        build_file_name(&movie(), &file(), &off).unwrap(),
        "30 Rock - S01E01 - Test"
    );
    let by_path = NamingFile {
        relative_path: None,
        path: Some("/movies/x/Some.File.mkv".into()),
        ..file()
    };
    assert_eq!(
        build_file_name(&movie(), &by_path, &off).unwrap(),
        "Some.File"
    );
}

#[test]
fn colon_replacement() {
    let cfg = |c| NamingConfig {
        rename_movies: true,
        standard_movie_format: "{Movie Title}".into(),
        colon_replacement: c,
        ..Default::default()
    };
    for (title, mode, expected) in [
        ("CSI: Vegas", ColonReplacement::Smart, "CSI - Vegas"),
        ("CSI: Vegas", ColonReplacement::Dash, "CSI- Vegas"),
        ("CSI: Vegas", ColonReplacement::Delete, "CSI Vegas"),
        ("CSI: Vegas", ColonReplacement::SpaceDash, "CSI - Vegas"),
        (
            "CSI: Vegas",
            ColonReplacement::SpaceDashSpace,
            "CSI - Vegas",
        ),
        ("Movie:Title", ColonReplacement::Smart, "Movie-Title"),
        ("Movie:Title", ColonReplacement::Dash, "Movie-Title"),
        ("Movie:Title", ColonReplacement::Delete, "MovieTitle"),
        ("Movie:Title", ColonReplacement::SpaceDash, "Movie -Title"),
        (
            "Movie:Title",
            ColonReplacement::SpaceDashSpace,
            "Movie - Title",
        ),
    ] {
        assert_eq!(
            build_file_name(&with_title(title), &file(), &cfg(mode)).unwrap(),
            expected,
            "{title} {mode:?}"
        );
    }
}

#[test]
fn clean_file_name_cases() {
    let c = NamingConfig::default();
    assert_eq!(
        clean_file_name("Mission: Impossible - no [HDTV-720p]", &c),
        "Mission - Impossible - no [HDTV-720p]"
    );
    assert_eq!(clean_file_name(".45 (2006)", &c), "45 (2006)");
    assert_eq!(clean_file_name(" The Movie Title ", &c), "The Movie Title");
}

#[test]
fn reserved_device_names() {
    for (title, expected) in [
        ("Con Game", "Con_Game (2021)"),
        ("Com1 Sat", "Com1_Sat (2021)"),
    ] {
        let m = NamingMovie {
            title: title.into(),
            year: 2021,
            ..Default::default()
        };
        assert_eq!(folder(&m, "{Movie.Title} ({Release Year})"), expected);
        assert_eq!(
            build_file_name(&m, &file(), &config("{Movie.Title} ({Release Year})")).unwrap(),
            expected
        );
    }
}

#[test]
fn truncation() {
    let m = with_title("The Fantastic Life of Mr. Sisko");
    assert_eq!(folder(&m, "{Movie Title:16}"), "The Fantastic...");
    assert_eq!(folder(&m, "{Movie TitleThe:17}"), "Fantastic Life...");
    assert_eq!(folder(&m, "{Movie CleanTitle:-13}"), "...Mr. Sisko");

    let long = "IWishIWasALittleBitTallerIWishIWasABallerIWishIHadAGirlWhoLookedGoodIWouldCallHerIWishIHadARabbitInAHatWithABatAndASixFourImpala";
    let f = NamingFile {
        release_group: Some(long.into()),
        quality_title: Some("Bluray-1080p".into()),
        ..file()
    };
    let m = NamingMovie { year: 2024, ..m };
    let n = |p: &str| build_file_name(&m, &f, &config(p)).unwrap();
    assert_eq!(
        n("{Movie Title} ({Release Year}) {Quality Full}-{ReleaseGroup:12}"),
        "The Fantastic Life of Mr. Sisko (2024) Bluray-1080p-IWishIWas..."
    );
    assert_eq!(
        n("{Movie Title} ({Release Year}) {Quality Full}-{ReleaseGroup:-17}"),
        "The Fantastic Life of Mr. Sisko (2024) Bluray-1080p-...ASixFourImpala"
    );
    // The marker survives an upper case token.
    assert_eq!(
        folder(
            &with_title("The Fantastic Life of Mr. Sisko"),
            "{MOVIE TITLE:16}"
        ),
        "THE FANTASTIC..."
    );
}

#[test]
fn id_tokens_and_tags() {
    let m = movie();
    assert_eq!(
        folder(&m, "{Movie Title} ({ImdbId})"),
        "South Park (tt1234567)"
    );
    assert_eq!(
        folder(&m, "{Movie Title} ({TmdbId})"),
        "South Park (124578)"
    );
    assert_eq!(
        folder(&m, "{Movie Title} {imdb-{ImdbId}}"),
        "South Park {imdb-tt1234567}"
    );
    assert_eq!(
        folder(&m, "{Movie Title} {{imdb-{ImdbId}}}"),
        "South Park {{imdb-tt1234567}}"
    );
    assert_eq!(
        folder(&m, "{Movie Title} {{tmdb-{TmdbId}}}"),
        "South Park {{tmdb-124578}}"
    );
    let none = NamingMovie {
        imdb_id: None,
        ..with_title("Movie Title")
    };
    for p in [
        "{Movie Title} {imdb-{ImdbId}}",
        "{Movie Title} {imdbid-{ImdbId}}",
        "{Movie Title} {{imdb-{ImdbId}}}",
        "{Movie Title} {{imdbid-{ImdbId}}}",
    ] {
        assert_eq!(folder(&none, p), "Movie Title", "{p}");
    }
}

#[test]
fn movie_folders_and_collections() {
    let m = |c: Option<&str>, t: &str, y| NamingMovie {
        title: t.into(),
        year: y,
        collection_title: c.map(Into::into),
        ..Default::default()
    };
    assert_eq!(
        folder(&m(None, "Arrival", 2016), "{Movie Title} ({Release Year})"),
        "Arrival (2016)"
    );
    assert_eq!(
        folder(
            &m(None, "The Big Short", 2015),
            "{Movie TitleThe} ({Release Year})"
        ),
        "Big Short, The (2015)"
    );
    assert_eq!(
        folder(
            &m(Some("The Y-Women Collection"), "The Y-Women 14", 2005),
            "{Movie CollectionThe}/{Movie TitleThe} ({Release Year})"
        ),
        "Y-Women Collection, The/Y-Women 14, The (2005)"
    );
    assert_eq!(
        folder(
            &m(Some("A Decade's Worth of Changes"), "The First Year", 1980),
            "{Movie CleanCollectionThe}/{Movie TitleThe} ({Release Year})"
        ),
        "Decades Worth of Changes, A/First Year, The (1980)"
    );
    assert_eq!(
        folder(
            &m(None, "Collectionless Slop", 1949),
            "{Movie CollectionThe}/{Movie TitleThe} ({Release Year})"
        ),
        "Collectionless Slop (1949)"
    );
}

#[test]
fn edition_tags() {
    let n = |edition: &str, p: &str| {
        let f = NamingFile {
            edition: Some(edition.into()),
            ..file()
        };
        build_file_name(&with_title("Movie Title"), &f, &config(p)).unwrap()
    };
    assert_eq!(
        n("Uncut", "{Movie Title} [{Edition Tags}]"),
        "Movie Title [Uncut]"
    );
    for p in [
        "{Movie Title} {Edition Tags}",
        "{Movie Title} {{Edition Tags}}",
        "{Movie Title} {edition-{Edition Tags}}",
        "{Movie Title} {{edition-{Edition Tags}}}",
    ] {
        assert_eq!(n("", p), "Movie Title", "{p}");
    }
    assert_eq!(
        n("Uncut", "{Movie Title} {{Edition Tags}}"),
        "Movie Title {Uncut}"
    );
    assert_eq!(
        n("Uncut", "{Movie Title} {{edition-{Edition Tags}}}"),
        "Movie Title {{edition-Uncut}}"
    );
    for (edition, pattern, expected) in [
        (
            "1st anniversary edition",
            "{Edition Tags}",
            "1st Anniversary Edition",
        ),
        (
            "4th anNiverSary eDitIOn",
            "{Edition Tags}",
            "4th Anniversary Edition",
        ),
        (
            "7TH anniversary edition",
            "{Edition Tags}",
            "7th Anniversary Edition",
        ),
        (
            "10Th anniversary edition",
            "{edition tags}",
            "10th anniversary edition",
        ),
        (
            "10Th anniversary edition",
            "{EDITION TAGS}",
            "10TH ANNIVERSARY EDITION",
        ),
        ("35MM EDITION", "{Edition Tags}", "35mm Edition"),
        ("35mm edition", "{edition tags}", "35mm edition"),
        ("imax", "{Edition Tags}", "IMAX"),
        ("Imax", "{edition tags}", "imax"),
    ] {
        assert_eq!(n(edition, pattern), expected, "{edition} / {pattern}");
    }
}

#[test]
fn media_info_tokens() {
    let media = |audio: &[&str], subs: &[&str]| NamingMedia {
        video_codec: "H264".into(),
        audio_codec: "DTS".into(),
        audio_languages: audio.iter().map(|s| s.to_string()).collect(),
        subtitles: subs.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    };
    let n = |m: NamingMedia, p: &str| {
        let f = NamingFile {
            media: Some(m),
            ..file()
        };
        build_file_name(&movie(), &f, &config(p)).unwrap()
    };
    assert_eq!(
        n(
            media(&["eng", "spa"], &["eng", "spa", "ita"]),
            "{Movie.Title}.{MEDIAINFO.FULL}"
        ),
        "South.Park.H264.DTS[EN+ES].[EN+ES+IT]"
    );
    assert_eq!(
        n(
            media(&["eng", "spa", "ita"], &[]),
            "{Movie.Title}.{MEDIAINFO.FULL}"
        ),
        "South.Park.H264.DTS[EN+ES+IT]"
    );
    let three_d = NamingMedia {
        is_3d: true,
        video_codec: "h264".into(),
        ..media(&[], &[])
    };
    assert_eq!(
        n(three_d, "{Movie.Title}.{MEDIAINFO.3D}.{MediaInfo.Simple}"),
        "South.Park.3D.h264.DTS"
    );
    assert_eq!(n(media(&["eng"], &[]), "{MediaInfo AudioCodec}"), "DTS");

    for (lang, code) in [
        ("nob", "NB"),
        ("swe", "SV"),
        ("zho", "ZH"),
        ("chi", "ZH"),
        ("fre", "FR"),
        ("rum", "RO"),
        ("per", "FA"),
        ("ger", "DE"),
        ("gsw", "DE"),
        ("cze", "CS"),
        ("ice", "IS"),
        ("dut", "NL"),
        ("nor", "NO"),
        ("khk", "MN"),
        ("mvf", "MN"),
        ("geo", "KA"),
        ("kat", "KA"),
    ] {
        assert_eq!(
            n(media(&[lang, "eng"], &[]), "{MediaInfo AudioLanguagesAll}"),
            format!("[{code}+EN]"),
            "{lang}"
        );
    }
}

#[test]
fn custom_format_tokens() {
    let f = NamingFile {
        custom_formats: vec!["Surround Sound".into(), "DTS-HD".into(), "Dolby".into()],
        ..file()
    };
    let n = |p: &str| build_file_name(&with_title("Movie Title"), &f, &config(p)).unwrap();
    assert_eq!(
        n("{Movie Title} {Custom Formats}"),
        "Movie Title Surround Sound DTS-HD Dolby"
    );
    assert_eq!(
        n("{Movie Title} {Custom Formats:DTS-HD,Dolby}"),
        "Movie Title DTS-HD Dolby"
    );
    assert_eq!(
        n("{Movie Title} {Custom Formats:-DTS-HD}"),
        "Movie Title Surround Sound Dolby"
    );
    assert_eq!(
        n("{Movie Title} {Custom Format:DTS-HD}"),
        "Movie Title DTS-HD"
    );
    assert_eq!(n("{Movie Title} {Custom Format:Nope}"), "Movie Title");
}

#[test]
fn path_patterns_make_folders_and_empty_format_is_an_error() {
    assert_eq!(
        name("{Movie Title} ({Release Year})/{Movie Title} - {Quality Title}"),
        "South Park (2020)/South Park - HDTV-720p"
    );
    assert!(build_file_name(&movie(), &file(), &config("  ")).is_err());
}
