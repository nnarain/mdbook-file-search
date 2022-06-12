//
// main.rs
//
// @author Natesh Narain <nnaraindev@gmail.com>
// @date Feb 22 2022
//
use mdbook::{
    book::Book,
    BookItem,
    errors::Result,
    preprocess::{CmdPreprocessor, Preprocessor, PreprocessorContext},
};
use semver::{Version, VersionReq};
// use toml::map::Map;
use clap::{Command, Arg, ArgMatches};

use regex::{Regex, Captures};


use std::io;
use std::fs;
use std::fmt;
use std::process;

use std::path::PathBuf;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug)]
enum FileSearchProcessorError {
    FileTypeConversionFailed,
}

impl fmt::Display for FileSearchProcessorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            FileSearchProcessorError::FileTypeConversionFailed => write!(f, "Invalid file type"),
        }
    }
}

impl std::error::Error for FileSearchProcessorError {}

#[derive(Clone, Copy)]
enum FileType {
    Link, Image,
}

impl TryFrom<&str> for FileType {
    type Error = FileSearchProcessorError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "image" => Ok(FileType::Image),
            "link" => Ok(FileType::Link),
            _ => Err(FileSearchProcessorError::FileTypeConversionFailed)
        }
    }
}

struct FileCache {
    output_dir: PathBuf,
    alias_to_path: HashMap<String, PathBuf>,
    alias_to_type: HashMap<String, FileType>,
}

impl FileCache {
    pub fn new(root: PathBuf) -> Result<FileCache> {
        let output_dir = root.join("src").join("_files");

        Ok(
            FileCache {
                output_dir,
                alias_to_path: Default::default(),
                alias_to_type: Default::default(),
            }
        )
    }

    pub fn copy_files(&self) -> Result<()> {
        if !self.output_dir.exists() {
            fs::create_dir(&self.output_dir)?;
        }

        for (_, path) in self.alias_to_path.iter() {
            if let Some(file_name) = path.file_name() {
                let output_file = self.output_dir.clone().join(file_name);

                // Copy the source file to the destination if it doesn't exist or if the existing file is out of date
                let should_copy = if output_file.exists() {
                    // Get modified time of the source and output file
                    let source_modified = fs::metadata(path)?.modified()?;
                    let output_modified = fs::metadata(output_file.clone())?.modified()?;

                    output_modified < source_modified
                }
                else {
                    true
                };

                if should_copy {
                    fs::copy(path, output_file)?;
                }
            }
        }

        Ok(())
    }

    pub fn get_insert_text(&self, alias: &str) -> Option<String> {
        let link = self.get_link_path(alias);
        let file_type = self.alias_to_type.get(alias);

        if let (Some(link), Some(file_type)) = (link, file_type) {
            match *file_type {
                FileType::Link => Some(format!("[{}]({})", alias, link)),
                FileType::Image => Some(format!("![Image not found]({})", link)),
            }
        }
        else {
            None
        }
    }

    pub fn get_link_path(&self, alias: &str) -> Option<String> {
        self.alias_to_path.get(alias).map(|path| {
            path
                .file_name()
                .map(|filename| filename.to_str())
                .flatten()
                .map(|filename| format!("./_files/{}", filename))
        }).flatten()
    }

    pub fn add_file(&mut self, alias: &str, path: &str, file_type: &str) -> Result<()> {
        self.alias_to_path.insert(alias.to_owned(), PathBuf::from(path));

        let file_type: FileType = file_type.try_into()?;
        self.alias_to_type.insert(alias.to_owned(), file_type);
        Ok(())
    }
}

#[derive(Default)]
struct FileSearch;

impl Preprocessor for FileSearch {
    fn name(&self) -> &str {
        "file-search"
    }

    fn run(&self, ctx: &PreprocessorContext, mut book: Book) -> Result<Book> {
        let mut cache = FileCache::new(PathBuf::from(ctx.root.clone()))?;

        // Load the file alias mapping from the supplied preprocessor configuration
        if let Some(cfg) = ctx.config.get_preprocessor(self.name()) {
            if let Some(toml::Value::Array(files)) = cfg.get("files") {
                for file in files.iter().filter_map(|item| item.as_table()) {
                    let alias = file.get("alias").map(|value| value.as_str()).flatten();
                    let path = file.get("path").map(|value| value.as_str()).flatten();
                    let file_type = file.get("type").map(|value| value.as_str()).flatten();

                    if let (Some(alias), Some(path), Some(file_type)) = (alias, path, file_type) {
                        cache.add_file(alias, path, file_type)?;
                    }
                }
            }
        }

        // Copy configured files into the output directory
        cache.copy_files()?;

        // Find any alias mappings in the chapter and substitute a direct link to a file that was copied from outside
        // the book directory

        // The alias syntax looks like: {{#find foo}}
        // Where `foo` is the alias defined in the `files` preprocessor
        let re = Regex::new(r"\{\{\#find\s([\d\w]+)\}\}")?;

        book.for_each_mut(move |item: &mut BookItem|{
            if let BookItem::Chapter(ref mut chapter) = item {
                chapter.content = re.replace_all(chapter.content.as_str(), |groups: &Captures| {
                    let alias = &groups[1];
                    // let link_path = cache.get_link_path(alias).unwrap_or("unknown".to_string());
                    // format!("[{}]({})", alias, link_path)
                    cache.get_insert_text(alias).unwrap_or("unknown".to_string())
                }).to_string();
            }
        });

        Ok(book)
    }

    fn supports_renderer(&self, _renderer: &str) -> bool {
        true
    }
}

fn main() {
    let matches = make_app().get_matches();

    let preprocessor = FileSearch::default();

    if let Some(args) = matches.subcommand_matches("supports") {
        handle_supports(&preprocessor, args);
    }
    else {
        if let Err(e) = handle_processing(&preprocessor) {
            eprintln!("Failed to process book: {}", e);
            process::exit(1);
        }
    }
}

fn handle_processing(preprocessor: &dyn Preprocessor) -> Result<()> {
    let (ctx, book) = CmdPreprocessor::parse_input(io::stdin())?;

    let book_version = Version::parse(&ctx.mdbook_version)?;
    let version_req = VersionReq::parse(mdbook::MDBOOK_VERSION)?;

    if !version_req.matches(&book_version) {
        eprintln!("Warning: The {} plugin was built against version {} of mdbook, but called for version {}",
            preprocessor.name(), mdbook::MDBOOK_VERSION, ctx.mdbook_version
        );
    }

    let processed_book = preprocessor.run(&ctx, book)?;
    serde_json::to_writer(io::stdout(), &processed_book)?;

    Ok(())
}

fn handle_supports(preprocessor: &dyn Preprocessor, args: &ArgMatches) -> ! {
    let renderer = args.value_of("renderer").expect("Required argument");
    let supported = preprocessor.supports_renderer(renderer);

    if supported {
        process::exit(0);
    }
    else {
        process::exit(1);
    }
}

fn make_app() -> Command<'static> {
    Command::new("file-search")
        .about("A mdbook preprocessor which find files outside of the book directory and links them")
        .subcommand(
            Command::new("supports")
                .arg(Arg::new("renderer").required(true))
                .about("Check whether a renderer is supported by this preprocessor"),
        )
}
