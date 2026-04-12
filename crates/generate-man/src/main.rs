use {
  pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag},
  std::{env, fs, io, io::Write, ops::Deref, process},
};

/// README H3 headings to skip (installation, editors, meta, etc.)
const SKIP_SECTIONS: &[&str] = &[
  "Prerequisites",
  "Packages",
  "Pre-Built Binaries",
  "GitHub Actions",
  "Release RSS Feed",
  "Node.js Installation",
  "Vim and Neovim",
  "Emacs",
  "Visual Studio Code",
  "JetBrains IDEs",
  "Kakoune",
  "Helix",
  "Sublime Text",
  "Micro",
  "Zed",
  "Other Editors",
  "Language Server Protocol",
  "Model Context Protocol",
  "Shell Completion Scripts",
  "Man Page",
  "just.sh",
  "Node.js package.json Script Compatibility",
  "Paths on Windows",
  "Alternatives and Prior Art",
  "Getting Started",
  "Contribution Workflow",
  "Hints",
  "Janus",
  "Minimum Supported Rust Version",
  "New Releases",
  "What are the idiosyncrasies of Make that Just avoids?",
  "What's the relationship between Just and Cargo build scripts?",
];

/// Map README H3 headings to manpage section names. Headings not listed here
/// become their own `.SH` using the heading text uppercased.
const SECTION_MAP: &[(&str, &str)] = &[
  ("The Default Recipe", "DESCRIPTION"),
  ("Listing Available Recipes", "DESCRIPTION"),
  ("Invoking Multiple Recipes", "DESCRIPTION"),
  ("Working Directory", "DESCRIPTION"),
  ("Aliases", "DESCRIPTION"),
  ("Settings", "SETTINGS"),
  ("Documentation Comments", "JUSTFILE SYNTAX"),
  ("Expressions and Substitutions", "EXPRESSIONS"),
  ("Strings", "EXPRESSIONS"),
  ("Ignoring Errors", "RECIPES"),
  ("Functions", "FUNCTIONS"),
  ("Constants", "EXPRESSIONS"),
  ("Attributes", "ATTRIBUTES"),
  ("Groups", "RECIPES"),
  ("Command Evaluation Using Backticks", "EXPRESSIONS"),
  ("Conditional Expressions", "EXPRESSIONS"),
  ("Stopping execution with error", "EXPRESSIONS"),
  ("Setting Variables from the Command Line", "VARIABLES"),
  ("Getting and Setting Environment Variables", "VARIABLES"),
  ("Recipe Parameters", "RECIPES"),
  ("Dependencies", "RECIPES"),
  ("Shebang Recipes", "RECIPES"),
  ("Script Recipes", "RECIPES"),
  ("Script and Shebang Recipe Temporary Files", "RECIPES"),
  ("Python Recipes with uv", "RECIPES"),
  ("Safer Bash Shebang Recipes", "RECIPES"),
  ("Setting Variables in a Recipe", "RECIPES"),
  ("Sharing Environment Variables Between Recipes", "RECIPES"),
  ("Changing the Working Directory in a Recipe", "RECIPES"),
  ("Indentation", "JUSTFILE SYNTAX"),
  ("Multi-Line Constructs", "JUSTFILE SYNTAX"),
  ("Command-line Options", "OPTIONS"),
  ("Private Recipes", "RECIPES"),
  ("Quiet Recipes", "RECIPES"),
  ("Selecting Recipes to Run With an Interactive Chooser", "OPTIONS"),
  ("Invoking justfiles in Other Directories", "OPTIONS"),
  ("Imports", "MODULES AND IMPORTS"),
  ("Modules", "MODULES AND IMPORTS"),
  ("Hiding justfiles", "FILES"),
  ("Just Scripts", "JUSTFILE SYNTAX"),
  ("Formatting and dumping justfiles", "OPTIONS"),
  ("Fallback to parent justfiles", "FILES"),
  ("Avoiding Argument Splitting", "RECIPES"),
  ("Configuring the Shell", "SETTINGS"),
  ("Timestamps", "OPTIONS"),
  ("Signal Handling", "SIGNALS"),
  ("Re-running recipes when files change", "OPTIONS"),
  ("Parallelism", "OPTIONS"),
  ("Shell Alias", "OPTIONS"),
  ("Remote Justfiles", "FILES"),
  ("Printing Complex Strings", "RECIPES"),
  ("Grammar", "GRAMMAR"),
  ("Global and User justfiles", "FILES"),
];

/// Ordered list of manpage sections to emit.
const SECTION_ORDER: &[&str] = &[
  "DESCRIPTION",
  "JUSTFILE SYNTAX",
  "SETTINGS",
  "RECIPES",
  "EXPRESSIONS",
  "FUNCTIONS",
  "VARIABLES",
  "ATTRIBUTES",
  "MODULES AND IMPORTS",
  "OPTIONS",
  "FILES",
  "SIGNALS",
  "GRAMMAR",
];

fn main() {
  let args: Vec<String> = env::args().collect();
  if args.len() != 2 {
    eprintln!("usage: generate-man <README.md>");
    process::exit(1);
  }

  let readme = fs::read_to_string(&args[1]).unwrap_or_else(|e| {
    eprintln!("cannot read {}: {e}", args[1]);
    process::exit(1);
  });

  let grammar = fs::read_to_string("GRAMMAR.md").unwrap_or_default();

  let sections = extract_sections(&readme);
  let mut out = io::stdout().lock();
  emit_manpage(&mut out, &sections, &grammar).expect("write failed");
}

/// A chunk of README content under one H3 heading.
struct Section<'a> {
  title: String,
  events: Vec<Event<'a>>,
}

/// Extract H3-delimited sections from README, skipping unwanted ones.
fn extract_sections(readme: &str) -> Vec<Section<'_>> {
  let options = Options::ENABLE_TABLES
    | Options::ENABLE_STRIKETHROUGH
    | Options::ENABLE_TASKLISTS;
  let parser = Parser::new_ext(readme, options);
  let mut sections: Vec<Section<'_>> = Vec::new();
  let mut current_events: Vec<Event<'_>> = Vec::new();
  let mut current_title = String::new();
  let mut in_heading = false;
  let mut heading_text = String::new();
  let mut heading_sup = false;
  let mut skip_depth: Option<HeadingLevel> = None;

  // Skip everything before the first H3 (badges, intro HTML).
  // We'll emit our own DESCRIPTION intro.
  let mut seen_first_h3 = false;

  for event in parser {
    match &event {
      Event::Start(Tag::Heading(HeadingLevel::H3, ..)) => {
        // Flush previous section
        if seen_first_h3 && skip_depth.is_none() && !current_events.is_empty() {
          sections.push(Section {
            title: current_title.clone(),
            events: std::mem::take(&mut current_events),
          });
        }
        in_heading = true;
        heading_text.clear();
        heading_sup = false;
        skip_depth = None;
      }
      Event::End(Tag::Heading(HeadingLevel::H3, ..)) => {
        in_heading = false;
        current_title = heading_text.clone();
        seen_first_h3 = true;

        if SKIP_SECTIONS.iter().any(|s| *s == current_title) {
          skip_depth = Some(HeadingLevel::H3);
          current_events.clear();
          continue;
        }
        current_events.clear();
      }
      Event::Html(html) if in_heading => {
        let html = html.deref().trim();
        if html.starts_with("<sup") {
          heading_sup = true;
        } else if html.starts_with("</sup") {
          heading_sup = false;
        }
        continue;
      }
      Event::Text(t) if in_heading => {
        if !heading_sup {
          heading_text.push_str(t.deref());
        }
        continue;
      }
      Event::Code(t) if in_heading => {
        if !heading_sup {
          heading_text.push_str(t.deref());
        }
        continue;
      }
      _ => {}
    }

    if skip_depth.is_some() {
      // Check if we've hit the next H3 which will reset skip_depth
      continue;
    }

    if seen_first_h3 && !in_heading {
      current_events.push(event);
    }
  }

  // Flush last section
  if skip_depth.is_none() && !current_events.is_empty() {
    sections.push(Section {
      title: current_title,
      events: current_events,
    });
  }

  sections
}

/// Look up which manpage section a README heading belongs to.
fn manpage_section_for(title: &str) -> Option<&'static str> {
  SECTION_MAP
    .iter()
    .find(|(t, _)| *t == title)
    .map(|(_, s)| *s)
}

/// Emit the complete manpage to the writer.
fn emit_manpage(
  w: &mut impl Write,
  sections: &[Section<'_>],
  grammar: &str,
) -> io::Result<()> {
  // Header
  writeln!(w, r#".TH JUST 1 "" "just" "General Commands Manual""#)?;

  // NAME
  writeln!(w, ".SH NAME")?;
  writeln!(w, r"just \- a command runner")?;

  // SYNOPSIS
  writeln!(w, ".SH SYNOPSIS")?;
  writeln!(w, r"\fBjust\fR [\fIOPTIONS\fR] [\fIRECIPE\fR [\fIARGUMENTS\fR...]]")?;

  // Group sections by manpage section
  let mut grouped: Vec<(&str, Vec<&Section<'_>>)> = Vec::new();
  for man_section in SECTION_ORDER {
    let matching: Vec<&Section<'_>> = sections
      .iter()
      .filter(|s| manpage_section_for(&s.title) == Some(man_section))
      .collect();
    if !matching.is_empty() {
      grouped.push((man_section, matching));
    }
  }

  // Collect unmapped sections
  let mapped_titles: Vec<&str> = SECTION_MAP.iter().map(|(t, _)| *t).collect();
  let unmapped: Vec<&Section<'_>> = sections
    .iter()
    .filter(|s| {
      !mapped_titles.contains(&s.title.as_str()) && !SKIP_SECTIONS.contains(&s.title.as_str())
    })
    .collect();

  // Emit DESCRIPTION intro
  writeln!(w, ".SH DESCRIPTION")?;
  writeln!(
    w,
    r"\fBjust\fR is a handy way to save and run project-specific commands."
  )?;
  writeln!(w, ".PP")?;
  writeln!(
    w,
    r"Commands, called recipes, are stored in a file called \fBjustfile\fR with syntax inspired by \fBmake\fR(1)."
  )?;

  // Emit grouped sections
  for (man_section, readme_sections) in &grouped {
    // DESCRIPTION content follows the intro we already wrote
    if *man_section != "DESCRIPTION" {
      writeln!(w, ".SH {man_section}")?;
    }

    for (i, section) in readme_sections.iter().enumerate() {
      if *man_section == "DESCRIPTION" && i == 0 {
        // First DESCRIPTION subsection — add spacing after intro
        writeln!(w, ".PP")?;
      }
      writeln!(w, r".SS {}", escape_roff(&section.title))?;
      emit_events(w, &section.events)?;
    }
  }

  // Emit unmapped sections
  for section in &unmapped {
    writeln!(w, ".SH {}", section.title.to_uppercase())?;
    emit_events(w, &section.events)?;
  }

  // GRAMMAR section from GRAMMAR.md
  if !grammar.is_empty() {
    // Only emit if not already covered by a mapped section
    let has_grammar = grouped.iter().any(|(s, _)| *s == "GRAMMAR");
    if has_grammar {
      // The mapped Grammar section from README just references GRAMMAR.md,
      // so replace its content with the actual grammar.
      // We already emitted the .SH GRAMMAR header above, so just add content.
    }
    emit_grammar(w, grammar)?;
  }

  // SEE ALSO
  writeln!(w, ".SH SEE ALSO")?;
  writeln!(
    w,
    r"\fBmake\fR(1), \fBjust\fR online manual: \fIhttps://just.systems/man/en/\fR"
  )?;

  // AUTHORS
  writeln!(w, ".SH AUTHORS")?;
  writeln!(w, "Casey Rodarmor <casey@rodarmor.com>")?;

  Ok(())
}

/// Convert a sequence of pulldown-cmark events to roff.
fn emit_events(w: &mut impl Write, events: &[Event<'_>]) -> io::Result<()> {
  let mut in_code_block = false;
  let mut in_list = false;
  let mut in_list_item = false;
  let mut list_item_first_para = false;
  let mut in_table = false;
  let mut table_row: Vec<String> = Vec::new();
  let mut table_header = true;
  let mut in_table_cell = false;
  let mut cell_text = String::new();
  let mut pending_text = String::new();
  let mut in_strong = false;
  let mut in_emphasis = false;
  let mut in_sup = false;
  let mut sup_text = String::new();

  for event in events {
    match event {
      Event::Start(Tag::Paragraph) => {
        if in_list_item {
          if !list_item_first_para {
            writeln!(w)?;
          }
        } else if !in_table {
          writeln!(w, ".PP")?;
        }
      }
      Event::End(Tag::Paragraph) => {
        if !pending_text.is_empty() {
          let text = std::mem::take(&mut pending_text);
          write!(w, "{}", escape_roff(&text))?;
        }
        if !in_table {
          writeln!(w)?;
        }
      }

      Event::Start(Tag::Heading(HeadingLevel::H4, ..)) => {
        pending_text.clear();
      }
      Event::End(Tag::Heading(HeadingLevel::H4, ..)) => {
        let text = std::mem::take(&mut pending_text);
        writeln!(w, ".SS {}", escape_roff(&text))?;
      }
      Event::Start(Tag::Heading(_, ..)) => {
        pending_text.clear();
      }
      Event::End(Tag::Heading(_, ..)) => {
        // H5+ headings become bold paragraphs
        let text = std::mem::take(&mut pending_text);
        writeln!(w, ".PP")?;
        writeln!(w, r"\fB{}\fR", escape_roff(&text))?;
      }

      Event::Start(Tag::CodeBlock(_)) => {
        in_code_block = true;
        writeln!(w, ".PP")?;
        writeln!(w, ".nf")?;
      }
      Event::End(Tag::CodeBlock(_)) => {
        in_code_block = false;
        writeln!(w, ".fi")?;
      }

      Event::Start(Tag::List(_)) => {
        in_list = true;
      }
      Event::End(Tag::List(_)) => {
        in_list = false;
      }

      Event::Start(Tag::Item) => {
        in_list_item = true;
        list_item_first_para = true;
        writeln!(w, r".IP \(bu 2")?;
      }
      Event::End(Tag::Item) => {
        if !pending_text.is_empty() {
          let text = std::mem::take(&mut pending_text);
          write!(w, "{}", escape_roff(&text))?;
        }
        writeln!(w)?;
        in_list_item = false;
      }

      Event::Start(Tag::Strong) => {
        if in_code_block || in_table_cell {
          // no formatting in preformatted blocks
        } else {
          flush_text(w, &mut pending_text)?;
          in_strong = true;
          write!(w, r"\fB")?;
        }
      }
      Event::End(Tag::Strong) => {
        if in_strong {
          flush_text(w, &mut pending_text)?;
          write!(w, r"\fR")?;
          in_strong = false;
        }
      }

      Event::Start(Tag::Emphasis) => {
        if !in_code_block && !in_table_cell {
          flush_text(w, &mut pending_text)?;
          in_emphasis = true;
          write!(w, r"\fI")?;
        }
      }
      Event::End(Tag::Emphasis) => {
        if in_emphasis {
          flush_text(w, &mut pending_text)?;
          write!(w, r"\fR")?;
          in_emphasis = false;
        }
      }

      Event::Start(Tag::Link(_, url, _)) => {
        if !in_code_block {
          flush_text(w, &mut pending_text)?;
        }
        // We'll collect the link text, then append URL
        let _ = url; // used in End(Link)
      }
      Event::End(Tag::Link(_, url, _)) => {
        if !in_code_block && !url.is_empty() && !url.starts_with('#') {
          flush_text(w, &mut pending_text)?;
          write!(w, " ({})", url.deref())?;
        }
      }

      // Tables
      Event::Start(Tag::Table(_)) => {
        in_table = true;
        table_header = true;
        writeln!(w, ".PP")?;
        writeln!(w, ".nf")?;
      }
      Event::End(Tag::Table(_)) => {
        in_table = false;
        writeln!(w, ".fi")?;
      }
      Event::Start(Tag::TableHead) => {
        table_row.clear();
      }
      Event::End(Tag::TableHead) => {
        let row = table_row.join("\t");
        writeln!(w, "{row}")?;
        table_header = false;
      }
      Event::Start(Tag::TableRow) => {
        table_row.clear();
      }
      Event::End(Tag::TableRow) => {
        let row = table_row.join("\t");
        writeln!(w, "{row}")?;
      }
      Event::Start(Tag::TableCell) => {
        in_table_cell = true;
        cell_text.clear();
      }
      Event::End(Tag::TableCell) => {
        in_table_cell = false;
        table_row.push(std::mem::take(&mut cell_text));
      }

      Event::Text(text) => {
        if in_sup {
          sup_text.push_str(text.deref());
        } else if in_table_cell {
          cell_text.push_str(text.deref());
        } else if in_code_block {
          for line in text.deref().split('\n') {
            let escaped = escape_roff_code(line);
            if escaped.starts_with('.') || escaped.starts_with('\'') {
              write!(w, "\\&{escaped}")?;
            } else {
              write!(w, "{escaped}")?;
            }
            writeln!(w)?;
          }
        } else {
          pending_text.push_str(text.deref());
        }
      }

      Event::Code(code) => {
        if in_table_cell {
          cell_text.push_str(code.deref());
        } else if in_code_block {
          write!(w, "{}", code.deref())?;
        } else {
          flush_text(w, &mut pending_text)?;
          write!(w, r"\fB{}\fR", escape_roff(code.deref()))?;
        }
      }

      Event::SoftBreak => {
        if in_code_block {
          writeln!(w)?;
        } else {
          pending_text.push(' ');
        }
      }

      Event::HardBreak => {
        flush_text(w, &mut pending_text)?;
        writeln!(w, ".br")?;
      }

      Event::Html(html) => {
        let html = html.deref().trim();
        if html.starts_with("<sup") {
          in_sup = true;
          sup_text.clear();
        } else if html.starts_with("</sup") {
          in_sup = false;
          if !sup_text.is_empty() {
            let annotation = format!(" (since {sup_text})");
            if in_table_cell {
              cell_text.push_str(&annotation);
            } else {
              pending_text.push_str(&annotation);
            }
            sup_text.clear();
          }
        }
        // Skip all other HTML (badges, div, br, h2, img, etc.)
      }

      Event::Start(Tag::BlockQuote) => {
        writeln!(w, ".RS 4")?;
      }
      Event::End(Tag::BlockQuote) => {
        writeln!(w, ".RE")?;
      }

      Event::Rule => {
        writeln!(w, ".PP")?;
      }

      _ => {}
    }

    if in_list_item && matches!(event, Event::End(Tag::Paragraph)) {
      list_item_first_para = false;
    }
  }

  Ok(())
}

/// Emit the GRAMMAR.md content as preformatted roff.
fn emit_grammar(w: &mut impl Write, grammar: &str) -> io::Result<()> {
  let parser = Parser::new_ext(grammar, Options::all());
  let mut in_code = false;
  let mut in_heading = false;

  for event in parser {
    match event {
      Event::Start(Tag::Heading(..)) => {
        in_heading = true;
      }
      Event::End(Tag::Heading(level, ..)) => {
        in_heading = false;
        let _ = level;
      }
      Event::Text(text) if in_heading => {
        writeln!(w, ".SS {}", escape_roff(text.deref()))?;
      }
      Event::Start(Tag::CodeBlock(_)) => {
        in_code = true;
        writeln!(w, ".PP")?;
        writeln!(w, ".nf")?;
      }
      Event::End(Tag::CodeBlock(_)) => {
        in_code = false;
        writeln!(w, ".fi")?;
      }
      Event::Text(text) if in_code => {
        for line in text.deref().split('\n') {
          let escaped = escape_roff_code(line);
          if escaped.starts_with('.') || escaped.starts_with('\'') {
            write!(w, "\\&{escaped}")?;
          } else {
            write!(w, "{escaped}")?;
          }
          writeln!(w)?;
        }
      }
      Event::Start(Tag::Paragraph) => {
        writeln!(w, ".PP")?;
      }
      Event::End(Tag::Paragraph) => {
        writeln!(w)?;
      }
      Event::SoftBreak => {
        write!(w, " ")?;
      }
      Event::Text(text) => {
        write!(w, "{}", escape_roff(text.deref()))?;
      }
      _ => {}
    }
  }

  Ok(())
}

fn flush_text(w: &mut impl Write, pending: &mut String) -> io::Result<()> {
  if !pending.is_empty() {
    let text = std::mem::take(pending);
    write!(w, "{}", escape_roff(&text))?;
  }
  Ok(())
}

/// Escape characters that are special in roff (prose text).
fn escape_roff(s: &str) -> String {
  s.replace('\\', "\\\\")
    .replace('-', "\\-")
    .replace('\'', "\\(aq")
}

/// Escape only backslashes in code blocks (inside .nf/.fi).
/// Hyphens and apostrophes are fine as-is in preformatted text.
fn escape_roff_code(s: &str) -> String {
  s.replace('\\', "\\\\")
}
