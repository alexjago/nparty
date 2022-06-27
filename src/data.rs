//! Functions to download preference data or print corresponding URLs.

use std::collections::BTreeMap;
use std::fs::{create_dir_all, write, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::utils::fetch_blocking;

// TODO: calamine for conversions...

// const STATES: [&str; 8] = ["ACT", "NT", "NSW", "QLD", "SA", "TAS", "VIC", "WA"];

/// The details of each election
#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct DlItems {
    year: String,
    id: String,
    polling_places: String,
    political_parties: String,
    sa1s_pps: String,
    candidates: String,
    /// state/territory : URL
    formal_prefs: BTreeMap<String, String>,
}

/// Returns `data_files/downloads.ron` as a BTreeMap
pub fn make_data() -> BTreeMap<String, DlItems> {
    ron::de::from_str::<BTreeMap<String, DlItems>>(include_str!("data_files/downloads.ron"))
        .unwrap()
}

/// Output a formatted HTML page detailing the downloads
fn make_html(texts: &BTreeMap<String, DlItems>) -> String {
    let template_html: &str = include_str!("data_files/data_template.html");
    let template_list: &str = include_str!("data_files/list_template.html");

    let mut content = String::new();

    for (year, item) in texts {
        let mut listy = String::new();
        listy.push_str(&format!(
            "<li><a href=\"{}\">{}</a></li>\n",
            item.polling_places, "Polling Places (nation-wide)"
        ));
        listy.push_str(&format!(
            "<li><a href=\"{}\">{}</a></li>\n",
            item.political_parties, "Votes by SA1 (nation-wide)"
        ));
        listy.push_str(&format!(
            "<li><a href=\"{}\">{}</a></li>\n",
            item.sa1s_pps, "Candidates (nation-wide)"
        ));
        listy.push_str(&format!(
            "<li><a href=\"{}\">{}</a></li>\n",
            item.candidates, "Political Parties (nation-wide)"
        ));
        for (state, url) in &item.formal_prefs {
            listy.push_str(&format!(
                "<li><a href=\"{}\">Formal Preferences for {}</a></li>\n",
                url, state
            ));
        }
        content.push_str(
            &template_list
                .replace("LIST_ITEMS", &listy)
                .replace("YEAR", year),
        );
    }

    String::from(template_html).replace("CONTENT", &content)
}

/// Print the HTML of the download links
pub fn examine_html(filey: &Path) {
    let sacred_texts = make_data();
    let mut output = File::create(filey).expect("Error creating file");
    output
        .write_all(make_html(&sacred_texts).as_bytes())
        .expect("Error writing file");
}

/// Print the download links as plain text
pub fn examine_txt() {
    let sacred_texts = make_data();
    // eprintln!("{:#?}", sacred_texts);
    for (_, item) in sacred_texts {
        println!(
            "{}\n{}\n{}\n{}",
            item.polling_places, item.political_parties, item.sa1s_pps, item.candidates
        );
        for (_, url) in item.formal_prefs {
            println!("{}", url);
        }
    }
}

/// Download all the links to `dldir`.
pub fn download(dldir: &Path) -> anyhow::Result<()> {
    let sacred_texts = make_data();

    let mut dldir = dldir;

    if !dldir.is_file() {
        create_dir_all(dldir).unwrap();
    } else {
        dldir = dldir.parent().unwrap();
    }

    let mut skips = 0;

    for (_, item) in sacred_texts {
        let year_dir = dldir.join(item.year);
        create_dir_all(&year_dir).unwrap();
        let mut all_urls: Vec<String> = vec![
            item.polling_places,
            item.political_parties,
            item.sa1s_pps,
            item.candidates,
        ];
        for (_, link) in item.formal_prefs {
            all_urls.push(link);
        }

        for link in all_urls {
            if let Ok(linkpath) = url::Url::parse(&link) {
                let aspath = PathBuf::from(linkpath.path());
                let mut dlto = PathBuf::from(&year_dir);
                dlto.push(&aspath.file_name().unwrap());
                // globfn omitted for now
                if !dlto.is_file() {
                    eprintln!("Downloading: {}", &dlto.display());

                    match fetch_blocking(&link) {
                        Ok(response) => write(&dlto, response.bytes)
                            .unwrap_or_else(|e| eprintln!("Error writing file: {:?}", e)),
                        Err(e) => eprintln!(
                            "Error downloading {:#?}:\n{}",
                            &aspath.file_name().unwrap(),
                            e
                        ),
                    };
                } else if dlto.is_file() {
                    skips += 1;
                }
            } else {
                eprintln!("Error parsing URL `{}`; skipping.", &link);
            }
        }
    }
    if skips == 0 {
        eprintln!("Done!");
    } else {
        eprintln!("Done! Skipped {} already-downloaded files.", skips);
    }
    Ok(())
}
