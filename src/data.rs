use std::path::{Path, PathBuf};
// use atty;
use std::collections::{BTreeMap, HashMap};
// use maplit;
use reqwest;
use ron;
use std::fs::{create_dir_all, write, File};
use std::io::{copy, Error, Result, Write};
use url;

// TODO: calamine for conversions...

// const STATES: [&str; 8] = ["ACT", "NT", "NSW", "QLD", "SA", "TAS", "VIC", "WA"];

#[derive(Deserialize, Debug)]
pub struct DlItems {
    year: String,
    id: String,
    polling_places: String,
    political_parties: String,
    sa1s_pps: String,
    candidates: String,
    formal_prefs: BTreeMap<String, String>,
}

pub fn make_data() -> HashMap<String, DlItems> {
    ron::de::from_str::<HashMap<String, DlItems>>(include_str!("data_files/downloads.ron")).unwrap()
}

fn make_html(texts: &HashMap<String, DlItems>) -> String {
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
                .replace("YEAR", &year),
        );
    }

    return String::from(template_html).replace("CONTENT", &content);
}

pub fn examine_html(filey: &Path) {
    let sacred_texts = make_data();
    let mut output = File::create(filey).expect("Error creating file");
    output
        .write(make_html(&sacred_texts).as_bytes())
        .expect("Error writing file");
}

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

pub fn download(dldir: &Path) {
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
        let mut all_urls: Vec<String> = Vec::new();
        all_urls.push(item.polling_places);
        all_urls.push(item.political_parties);
        all_urls.push(item.sa1s_pps);
        all_urls.push(item.candidates);
        for (_, link) in item.formal_prefs {
            all_urls.push(link);
        }

        for link in all_urls {
            let linkpath = url::Url::parse(&link).unwrap();
            let aspath = PathBuf::from(linkpath.path());
            let mut dlto = PathBuf::from(&year_dir);
            dlto.push(&aspath.file_name().unwrap());
            // globfn omitted for now
            if !dlto.is_file() {
                eprintln!("Downloading: {}", &dlto.display());
                let response = reqwest::blocking::get(&link)
                    .expect(&format!(
                        "Error downloading {:#?}",
                        &aspath.file_name().unwrap()
                    ))
                    .bytes()
                    .unwrap();
                write(&dlto, response).expect("Error writing file");
            } else if dlto.is_file() {
                skips += 1;
            }
        }
    }
    if skips == 0 {
        eprintln!("Done!");
    } else {
        eprintln!("Done! Skipped {} already-downloaded files.", skips);
    }
}
