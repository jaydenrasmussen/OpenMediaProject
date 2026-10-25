//! The built-in usenet client: NZB parsing, yEnc decoding, NNTP, assembly,
//! repair and extraction.

mod client;
mod download;
mod nntp;
mod nzb;
mod post;
mod yenc;

pub use client::{UsenetClient, UsenetSettings};
pub use download::{Control, DownloadReport, FileReport, Limiter, download, safe_file_name};
pub use nntp::{NntpConnection, NntpError, ServerConfig};
pub use nzb::{Nzb, NzbError, NzbFile, NzbSegment};
pub use post::{
    ExtractReport, Par2Outcome, Tools, archives_to_extract, extract_archives, main_par2,
    par2_repair, remove_par2,
};
pub use yenc::{YencError, YencPart, decode_part, encode_part};
