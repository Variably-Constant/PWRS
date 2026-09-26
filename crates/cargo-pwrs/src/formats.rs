//! Format.ps1xml with one default view per output class: a table when
//! the class has at most five fields, a list otherwise.

use crate::descriptor::Module;

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

pub fn format_ps1xml(m: &Module) -> Option<String> {
    let classes: Vec<&crate::descriptor::Class> = m.classes.iter().filter(|c| c.mode != "enum").collect();
    if classes.is_empty() {
        return None;
    }
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<Configuration>\n  <ViewDefinitions>\n");
    for c in classes {
        let type_name = xml(&c.name);
        s.push_str("    <View>\n");
        s.push_str(&format!("      <Name>{type_name}</Name>\n"));
        s.push_str(&format!("      <ViewSelectedBy><TypeName>{type_name}</TypeName></ViewSelectedBy>\n"));
        if c.fields.len() <= 5 {
            s.push_str("      <TableControl>\n        <TableHeaders>\n");
            for f in &c.fields {
                s.push_str(&format!("          <TableColumnHeader><Label>{}</Label></TableColumnHeader>\n", xml(&f.name)));
            }
            s.push_str("        </TableHeaders>\n        <TableRowEntries>\n          <TableRowEntry>\n            <TableColumnItems>\n");
            for f in &c.fields {
                s.push_str(&format!("              <TableColumnItem><PropertyName>{}</PropertyName></TableColumnItem>\n", xml(&f.name)));
            }
            s.push_str("            </TableColumnItems>\n          </TableRowEntry>\n        </TableRowEntries>\n      </TableControl>\n");
        } else {
            s.push_str("      <ListControl>\n        <ListEntries>\n          <ListEntry>\n            <ListItems>\n");
            for f in &c.fields {
                s.push_str(&format!("              <ListItem><PropertyName>{}</PropertyName></ListItem>\n", xml(&f.name)));
            }
            s.push_str("            </ListItems>\n          </ListEntry>\n        </ListEntries>\n      </ListControl>\n");
        }
        s.push_str("    </View>\n");
    }
    s.push_str("  </ViewDefinitions>\n</Configuration>\n");
    Some(s)
}
