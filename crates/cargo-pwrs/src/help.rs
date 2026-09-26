//! MAML help generated from the descriptor's doc comments, in the
//! shape `Get-Help` reads for binary modules.

use crate::descriptor::{Cmdlet, Module, Param};

fn xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

fn paras(text: &str) -> String {
    let mut out = String::new();
    for block in text.split("\n\n") {
        let t = block.trim();
        if !t.is_empty() {
            out.push_str(&format!("<maml:para>{}</maml:para>", xml(&t.replace('\n', " "))));
        }
    }
    if out.is_empty() {
        out.push_str("<maml:para></maml:para>");
    }
    out
}

/// Splits a description into prose and the lines under a
/// `# Examples` heading, one example per non-empty line.
fn split_examples(description: &str) -> (String, Vec<String>) {
    let mut prose = Vec::new();
    let mut examples = Vec::new();
    let mut in_examples = false;
    for line in description.lines() {
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case("# examples") || trimmed.eq_ignore_ascii_case("# example") {
            in_examples = true;
            continue;
        }
        if in_examples {
            // The fence is recognized before the backticks come off,
            // so an opening ```powershell does not survive as an
            // example reading `powershell`.
            if trimmed.starts_with("```") {
                continue;
            }
            let code = trimmed.trim_matches('`').trim();
            if !code.is_empty() {
                examples.push(code.to_string());
            }
        } else {
            prose.push(line);
        }
    }
    (prose.join("\n"), examples)
}

fn pipeline_input(p: &Param) -> &'static str {
    match (p.pipeline, p.pipeline_by_name) {
        (true, true) => "True (ByValue, ByPropertyName)",
        (true, false) => "True (ByValue)",
        (false, true) => "True (ByPropertyName)",
        (false, false) => "False",
    }
}

fn param_xml(p: &Param, with_type: bool) -> String {
    let position = match p.position {
        Some(pos) => pos.to_string(),
        None => "named".to_string(),
    };
    let clr = if p.clr == "SwitchParameter" { "SwitchParameter".to_string() } else { p.clr.clone() };
    let mut s = format!(
        "<command:parameter required=\"{}\" variableLength=\"false\" globbing=\"false\" pipelineInput=\"{}\" position=\"{}\" aliases=\"{}\">",
        p.mandatory,
        pipeline_input(p),
        position,
        xml(&p.aliases.join(", "))
    );
    s.push_str(&format!("<maml:name>{}</maml:name>", xml(&p.name)));
    s.push_str(&format!("<maml:description>{}</maml:description>", paras(&p.help)));
    if p.clr != "SwitchParameter" {
        s.push_str(&format!("<command:parameterValue required=\"true\" variableLength=\"false\">{}</command:parameterValue>", xml(&clr)));
    }
    if with_type {
        s.push_str(&format!("<dev:type><maml:name>{}</maml:name><maml:uri /></dev:type><dev:defaultValue>None</dev:defaultValue>", xml(&clr)));
    }
    s.push_str("</command:parameter>");
    s
}

/// The parameter sets a cmdlet's syntax is written for: the default set
/// first, then each set in the order a parameter first names it. Empty
/// when neither the cmdlet nor any parameter names one.
fn syntax_sets(c: &Cmdlet) -> Vec<&str> {
    let mut sets: Vec<&str> = c.default_set.as_deref().into_iter().collect();
    for set in c.params.iter().flat_map(|p| p.sets.iter()) {
        if !sets.contains(&set.as_str()) {
            sets.push(set);
        }
    }
    sets
}

/// One syntax line: the parameters of `set` with those in every set, or
/// every parameter when `set` is `None`.
fn syntax_item(s: &mut String, c: &Cmdlet, set: Option<&str>) {
    s.push_str("<command:syntaxItem>");
    s.push_str(&format!("<maml:name>{}</maml:name>", xml(&c.name)));
    for p in &c.params {
        let belongs = match set {
            Some(set) => p.sets.is_empty() || p.sets.iter().any(|named| named == set),
            None => true,
        };
        if belongs {
            s.push_str(&param_xml(p, false));
        }
    }
    s.push_str("</command:syntaxItem>");
}

fn cmdlet_xml(c: &Cmdlet) -> String {
    let (prose, examples) = split_examples(&c.description);
    let mut s = String::new();
    s.push_str("<command:command xmlns:maml=\"http://schemas.microsoft.com/maml/2004/10\" xmlns:command=\"http://schemas.microsoft.com/maml/dev/command/2004/10\" xmlns:dev=\"http://schemas.microsoft.com/maml/dev/2004/10\">");
    s.push_str("<command:details>");
    s.push_str(&format!("<command:name>{}</command:name>", xml(&c.name)));
    s.push_str(&format!("<command:verb>{}</command:verb><command:noun>{}</command:noun>", xml(&c.verb), xml(&c.noun)));
    s.push_str(&format!("<maml:description>{}</maml:description>", paras(&c.synopsis)));
    s.push_str("</command:details>");
    s.push_str(&format!("<maml:description>{}</maml:description>", paras(if prose.trim().is_empty() { &c.synopsis } else { &prose })));

    s.push_str("<command:syntax>");
    let sets = syntax_sets(c);
    if sets.is_empty() {
        syntax_item(&mut s, c, None);
    }
    for set in sets {
        syntax_item(&mut s, c, Some(set));
    }
    s.push_str("</command:syntax>");

    s.push_str("<command:parameters>");
    for p in &c.params {
        s.push_str(&param_xml(p, true));
    }
    s.push_str("</command:parameters>");

    s.push_str("<command:inputTypes>");
    for p in c.params.iter().filter(|p| p.pipeline || p.pipeline_by_name) {
        s.push_str(&format!(
            "<command:inputType><dev:type><maml:name>{}</maml:name></dev:type><maml:description>{}</maml:description></command:inputType>",
            xml(&p.clr),
            paras(&p.help)
        ));
    }
    s.push_str("</command:inputTypes>");

    s.push_str("<command:returnValues>");
    for t in &c.output_types {
        s.push_str(&format!("<command:returnValue><dev:type><maml:name>{}</maml:name></dev:type><maml:description><maml:para></maml:para></maml:description></command:returnValue>", xml(t)));
    }
    s.push_str("</command:returnValues>");

    s.push_str("<command:examples>");
    for (i, ex) in examples.iter().enumerate() {
        s.push_str(&format!(
            "<command:example><maml:title>-------------------------- EXAMPLE {} --------------------------</maml:title><dev:code>{}</dev:code><dev:remarks><maml:para></maml:para></dev:remarks></command:example>",
            i + 1,
            xml(ex)
        ));
    }
    s.push_str("</command:examples>");
    s.push_str("</command:command>");
    s
}

/// The whole help document for the module's shell assembly.
pub fn maml(m: &Module) -> String {
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<helpItems schema=\"maml\" xmlns=\"http://msh\">\n");
    for c in &m.cmdlets {
        s.push_str(&cmdlet_xml(c));
        s.push('\n');
    }
    s.push_str("</helpItems>\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_description_without_the_heading_is_all_prose() {
        let (prose, examples) = split_examples("One line.\n\nA second paragraph.");
        assert_eq!(prose, "One line.\n\nA second paragraph.");
        assert!(examples.is_empty());
    }

    #[test]
    fn the_heading_ends_the_prose_and_each_line_after_it_is_an_example() {
        let (prose, examples) = split_examples("Does a thing.\n\n# Examples\nGet-Thing -Name a\n\n'x' | Get-Thing\n");
        assert_eq!(prose, "Does a thing.\n");
        assert_eq!(examples, vec!["Get-Thing -Name a".to_string(), "'x' | Get-Thing".to_string()]);
    }

    #[test]
    fn the_heading_is_matched_whatever_its_case_and_in_the_singular() {
        for heading in ["# Examples", "# examples", "# EXAMPLES", "# Example"] {
            let (_prose, examples) = split_examples(&format!("Does a thing.\n{heading}\nGet-Thing"));
            assert_eq!(examples, vec!["Get-Thing".to_string()], "heading {heading}");
        }
    }

    #[test]
    fn a_fenced_or_backticked_example_loses_its_markup() {
        let (_prose, examples) = split_examples("Does a thing.\n# Examples\n```powershell\n`Get-Thing -Name a`\n```");
        assert_eq!(examples, vec!["Get-Thing -Name a".to_string()]);
    }

    /// A cmdlet whose parameters are `(name, sets)`, with `default_set`.
    fn cmdlet(default_set: Option<&str>, params: &[(&str, &[&str])]) -> Cmdlet {
        let params: Vec<String> = params
            .iter()
            .enumerate()
            .map(|(i, (name, sets))| {
                format!(
                    r#"{{"name": "{name}", "rust": "p{i}", "index": {i}, "clr": "System.String", "slot": "str16", "optional": true,
                        "mandatory": false, "position": null, "sets": {sets:?}, "pipeline": false, "pipeline_by_name": false,
                        "remaining": false, "aliases": [], "help": "", "validate_set": [], "validate_range": null,
                        "validate_pattern": null, "not_null_or_empty": false, "dont_show": false, "literal_path": false}}"#
                )
            })
            .collect();
        let default_set = match default_set {
            Some(d) => format!("\"{d}\""),
            None => "null".to_string(),
        };
        let json = format!(
            r#"{{"id": 0, "verb": "Get", "noun": "Route", "name": "Get-Route", "rust": "GetRoute", "should_process": false,
                "confirm_impact": null, "default_set": {default_set}, "aliases": [], "output_types": [], "synopsis": "",
                "description": "", "params": [{}]}}"#,
            params.join(",")
        );
        serde_json::from_str(&json).expect("cmdlet")
    }

    /// The parameter names on each syntax line of the cmdlet's help.
    fn syntax_lines(c: &Cmdlet) -> Vec<Vec<String>> {
        let xml = cmdlet_xml(c);
        let syntax = &xml[xml.find("<command:syntax>").expect("syntax")..xml.find("</command:syntax>").expect("syntax end")];
        syntax
            .split("<command:syntaxItem>")
            .skip(1)
            .map(|item| item.split("<command:parameter ").skip(1).map(|p| p[p.find("<maml:name>").expect("name") + 11..p.find("</maml:name>").expect("name end")].to_string()).collect())
            .collect()
    }

    #[test]
    fn each_set_gets_a_syntax_line_of_its_own_parameters_the_default_first() {
        let c = cmdlet(
            Some("Path"),
            &[("Path", &["Path"]), ("LiteralPath", &["LiteralPath"]), ("Text", &["Text"]), ("Destination", &["Path", "LiteralPath"]), ("Quiet", &[])],
        );
        assert_eq!(
            syntax_lines(&c),
            vec![
                vec!["Path", "Destination", "Quiet"],
                vec!["LiteralPath", "Destination", "Quiet"],
                vec!["Text", "Quiet"],
            ]
        );
    }

    #[test]
    fn a_cmdlet_naming_no_set_has_one_syntax_line_of_every_parameter() {
        let c = cmdlet(None, &[("Name", &[]), ("Count", &[])]);
        assert_eq!(syntax_lines(&c), vec![vec!["Name", "Count"]]);
    }

    #[test]
    fn examples_reach_the_generated_maml() {
        let maml = split_examples("Does a thing.\n# Examples\nGet-Thing -Name a");
        assert_eq!(maml.1.len(), 1);
        let escaped = xml("Get-Thing -Name 'a' & b <c>");
        assert!(!escaped.contains('&') || escaped.contains("&amp;"), "{escaped}");
        assert!(!escaped.contains('<'), "{escaped}");
    }
}
