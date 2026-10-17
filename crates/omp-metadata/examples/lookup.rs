//! Try the TMDB provider with the token from `.env` (TMDB_READ_TOKEN).
//!
//!   cargo run -p omp-metadata --example lookup -- 603          # by TMDB id
//!   cargo run -p omp-metadata --example lookup -- tt0133093    # by IMDb id
//!   cargo run -p omp-metadata --example lookup -- matrix 1999  # search (+ year)

use anyhow::{Context, Result};
use omp_metadata::{MovieMetadataProvider, tmdb::TmdbProvider};

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let tmdb = TmdbProvider::from_env().context("set TMDB_READ_TOKEN in .env")?;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let first = args
        .first()
        .context("usage: lookup <tmdb id | imdb id | search term> [year]")?;

    if let Ok(id) = first.parse::<i64>() {
        print_movie(&tmdb.movie(id).await?);
    } else if first.starts_with("tt") {
        print_movie(&tmdb.movie_by_imdb_id(first).await?);
    } else {
        let year = args.get(1).and_then(|y| y.parse().ok());
        for m in tmdb.search(first, year).await?.iter().take(8) {
            println!(
                "{:>8}  {}  ({})  {:?}",
                m.tmdb_id, m.title, m.year, m.status
            );
        }
    }
    Ok(())
}

fn print_movie(m: &omp_core::movie::MovieMetadata) {
    let date = |d: Option<chrono::DateTime<chrono::Utc>>| {
        d.map_or("-".to_owned(), |d| d.format("%Y-%m-%d").to_string())
    };
    println!(
        "{} ({})  tmdb:{}  imdb:{}",
        m.title,
        m.year,
        m.tmdb_id,
        m.imdb_id.as_deref().unwrap_or("-")
    );
    println!(
        "status: {:?}   runtime: {} min   certification: {}",
        m.status,
        m.runtime,
        m.certification.as_deref().unwrap_or("-")
    );
    println!(
        "cinemas: {}   digital: {}   physical: {}",
        date(m.in_cinemas),
        date(m.digital_release),
        date(m.physical_release)
    );
    println!("genres: {}", m.genres.join(", "));
    if let Some(c) = &m.collection_title {
        println!("collection: {c}");
    }
    println!(
        "alt titles: {}   credits: {}   images: {}",
        m.alternative_titles.len(),
        m.credits.len(),
        m.images.len()
    );
}
