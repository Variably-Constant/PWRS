//! Format.ps1xml with the default views of the output classes. Every
//! class gets a table of its fields when it has at most five and a list
//! of them otherwise. A class that names its columns gets a table of
//! those ahead of that, and a class that names a view method gets a
//! custom view writing the text the method returns ahead of both. The
//! first view is the class's default, and Format-Table and Format-List
//! each take the first of their own kind.

use crate::descriptor::{Class, Module};

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Opens the view `name` for the class `type_name`, both already escaped.
fn open_view(s: &mut String, name: &str, type_name: &str) {
    s.push_str("    <View>\n");
    s.push_str(&format!("      <Name>{name}</Name>\n"));
    s.push_str(&format!("      <ViewSelectedBy><TypeName>{type_name}</TypeName></ViewSelectedBy>\n"));
}

fn table(s: &mut String, properties: &[&str]) {
    s.push_str("      <TableControl>\n        <TableHeaders>\n");
    for p in properties {
        s.push_str(&format!("          <TableColumnHeader><Label>{}</Label></TableColumnHeader>\n", xml(p)));
    }
    s.push_str("        </TableHeaders>\n        <TableRowEntries>\n          <TableRowEntry>\n            <TableColumnItems>\n");
    for p in properties {
        s.push_str(&format!("              <TableColumnItem><PropertyName>{}</PropertyName></TableColumnItem>\n", xml(p)));
    }
    s.push_str("            </TableColumnItems>\n          </TableRowEntry>\n        </TableRowEntries>\n      </TableControl>\n");
}

fn list(s: &mut String, properties: &[&str]) {
    s.push_str("      <ListControl>\n        <ListEntries>\n          <ListEntry>\n            <ListItems>\n");
    for p in properties {
        s.push_str(&format!("              <ListItem><PropertyName>{}</PropertyName></ListItem>\n", xml(p)));
    }
    s.push_str("            </ListItems>\n          </ListEntry>\n        </ListEntries>\n      </ListControl>\n");
}

pub fn format_ps1xml(m: &Module) -> Option<String> {
    let classes: Vec<&Class> = m.classes.iter().filter(|c| c.mode != "enum").collect();
    if classes.is_empty() {
        return None;
    }
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<Configuration>\n  <ViewDefinitions>\n");
    for c in classes {
        let type_name = xml(&c.name);
        if let Some(method) = &c.view {
            open_view(&mut s, &format!("{type_name}.{}", xml(method)), &type_name);
            s.push_str("      <CustomControl>\n        <CustomEntries>\n          <CustomEntry>\n            <CustomItem>\n");
            s.push_str(&format!("              <ExpressionBinding><ScriptBlock>$_.{}()</ScriptBlock></ExpressionBinding>\n", xml(method)));
            s.push_str("            </CustomItem>\n          </CustomEntry>\n        </CustomEntries>\n      </CustomControl>\n");
            s.push_str("    </View>\n");
        }
        if !c.columns.is_empty() {
            open_view(&mut s, &format!("{type_name}.Columns"), &type_name);
            let columns: Vec<&str> = c.columns.iter().map(String::as_str).collect();
            table(&mut s, &columns);
            s.push_str("    </View>\n");
        }
        open_view(&mut s, &type_name, &type_name);
        let fields: Vec<&str> = c.fields.iter().map(|f| f.name.as_str()).collect();
        if fields.len() <= 5 {
            table(&mut s, &fields);
        } else {
            list(&mut s, &fields);
        }
        s.push_str("    </View>\n");
    }
    s.push_str("  </ViewDefinitions>\n</Configuration>\n");
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::format_ps1xml;

    const FIELD: &str = r#"{"name": "Lines", "rust": "lines", "index": 0, "clr": "long", "slot": "i64", "optional": false, "help": ""}"#;

    #[test]
    fn a_class_that_names_a_view_method_is_shown_by_it_and_keeps_its_table() {
        let json = format!(
            r#"{{"abi": 1, "name": "M", "cmdlets": [], "classes": [
                {{"id": 1, "name": "M.Diff", "rust": "Diff", "mode": "proxy", "view": "Render", "description": "", "fields": [{FIELD}]}},
                {{"id": 2, "name": "M.Plain", "rust": "Plain", "mode": "copied", "description": "", "fields": [{FIELD}]}}
            ]}}"#
        );
        let module: crate::descriptor::Module = serde_json::from_str(&json).expect("a descriptor");
        let xml = format_ps1xml(&module).expect("a format file");

        let custom = xml.find("<ScriptBlock>$_.Render()</ScriptBlock>").expect("the custom view calls the method");
        let table = xml.find("<Name>M.Diff</Name>").expect("the class keeps its table view");
        assert!(custom < table, "the custom view comes first, so it is the default:\n{xml}");
        assert!(xml.contains("<Name>M.Diff.Render</Name>"), "{xml}");
        assert_eq!(xml.matches("<CustomControl>").count(), 1, "only the class that names a view gets one:\n{xml}");
        assert_eq!(xml.matches("<TableControl>").count(), 2, "{xml}");
    }

    /// A copied class named `name` with one long field per name in
    /// `fields`, naming `columns`.
    fn class(id: u32, name: &str, fields: &[&str], columns: &[&str]) -> String {
        let fields: Vec<String> = fields
            .iter()
            .map(|f| format!(r#"{{"name": "{f}", "rust": "f", "index": 0, "clr": "long", "slot": "i64", "optional": false, "help": ""}}"#))
            .collect();
        let columns: Vec<String> = columns.iter().map(|c| format!("\"{c}\"")).collect();
        format!(
            r#"{{"id": {id}, "name": "{name}", "rust": "R{id}", "mode": "copied", "columns": [{}], "description": "", "fields": [{}]}}"#,
            columns.join(", "),
            fields.join(", ")
        )
    }

    /// The views selecting `type_name`, in the order the file gives them.
    fn views_of<'a>(xml: &'a str, type_name: &str) -> Vec<&'a str> {
        let selected = format!("<ViewSelectedBy><TypeName>{type_name}</TypeName></ViewSelectedBy>");
        xml.split("    <View>\n").skip(1).filter(|v| v.contains(&selected)).collect()
    }

    /// The text of every `<tag>` in `view`, in order.
    fn texts(view: &str, tag: &str) -> Vec<String> {
        let open = format!("<{tag}>");
        view.split(open.as_str()).skip(1).map(|p| p[..p.find('<').expect("a closed element")].to_string()).collect()
    }

    #[test]
    fn a_class_that_names_columns_is_shown_first_by_a_table_of_them_and_keeps_its_view() {
        let seven = ["Name", "Items", "Done", "Failed", "Ms", "Workers", "Host"];
        let json = format!(
            r#"{{"abi": 1, "name": "M", "cmdlets": [], "classes": [{}, {}, {}]}}"#,
            class(1, "M.Run", &seven, &["Items", "Done", "Name"]),
            class(2, "M.Pair", &["Left", "Right"], &["Right"]),
            class(3, "M.Plain", &seven, &[]),
        );
        let module: crate::descriptor::Module = serde_json::from_str(&json).expect("a descriptor");
        let xml = format_ps1xml(&module).expect("a format file");

        let run = views_of(&xml, "M.Run");
        assert_eq!(run.len(), 2, "{xml}");
        assert!(run[0].starts_with("      <Name>M.Run.Columns</Name>\n"), "the table of named columns comes first, so it is the default:\n{xml}");
        assert!(run[0].contains("<TableControl>"), "{xml}");
        assert_eq!(texts(run[0], "Label"), ["Items", "Done", "Name"], "exactly the named columns, in their order");
        assert_eq!(texts(run[0], "PropertyName"), ["Items", "Done", "Name"]);
        assert!(run[1].starts_with("      <Name>M.Run</Name>\n") && run[1].contains("<ListControl>"), "the list of every field stays, second:\n{xml}");
        assert_eq!(texts(run[1], "PropertyName"), seven);

        let pair = views_of(&xml, "M.Pair");
        assert_eq!(pair.len(), 2, "{xml}");
        assert_eq!(texts(pair[0], "PropertyName"), ["Right"]);
        assert!(pair[1].starts_with("      <Name>M.Pair</Name>\n") && pair[1].contains("<TableControl>"), "a class of five fields or fewer keeps its table of them:\n{xml}");
        assert_eq!(texts(pair[1], "PropertyName"), ["Left", "Right"]);

        let plain = views_of(&xml, "M.Plain");
        assert_eq!(plain.len(), 1, "{xml}");
        let mut unchanged = String::from(
            "      <Name>M.Plain</Name>\n      <ViewSelectedBy><TypeName>M.Plain</TypeName></ViewSelectedBy>\n      <ListControl>\n        <ListEntries>\n          <ListEntry>\n            <ListItems>\n",
        );
        for f in seven {
            unchanged.push_str(&format!("              <ListItem><PropertyName>{f}</PropertyName></ListItem>\n"));
        }
        unchanged.push_str("            </ListItems>\n          </ListEntry>\n        </ListEntries>\n      </ListControl>\n    </View>\n");
        assert!(plain[0].starts_with(&unchanged), "a class naming no columns has its one list:\n{xml}");
    }
}
