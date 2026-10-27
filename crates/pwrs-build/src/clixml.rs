//! Windows PowerShell's CLIXML on an error stream, as plain lines.
//!
//! A PowerShell host started with `-EncodedCommand` writes its error,
//! warning, verbose, debug, information and progress records to its error
//! stream as CLIXML when that stream is a pipe or a file: Windows
//! PowerShell always, pwsh unless it is given `-OutputFormat Text`. The
//! stream then has a `#< CLIXML` line, `<Objs ...>`, an element for each
//! record and `</Objs>`, with no line break after it. The host writes the
//! document in pieces of about 6 KB as it grows and the rest when it
//! exits, so text that another writer puts on the stream in the meantime
//! lands between two pieces, in the middle of a tag or a word. A
//! PowerShell reading the stream with `2>&1` takes what follows a
//! `#< CLIXML` line for CLIXML, and drops a plain line that shares a line
//! with the document.
//!
//! [`Decoder`] passes plain text on as it arrives and leaves out each
//! `#< CLIXML` line. It writes each record as the host writes it when it
//! is not writing CLIXML: an error's lines as they are, and a warning,
//! verbose or debug record as its text after `WARNING: `, `VERBOSE: ` or
//! `DEBUG: `. It leaves out progress records, and information records,
//! which the host also writes to its output stream.
//!
//! CLIXML escapes each line break in its text, so a line break inside a
//! document ends text that another writer put there. That text is written
//! as a line of its own, from where the read that brought it began, or,
//! when it came in one read with part of the document, from the end of
//! the last whole tag before it.

/// The line a host writes before its first record, less its line break.
const MARKER: &[u8] = b"#< CLIXML";

/// The start of a document's root element.
const ROOT: &[u8] = b"<Objs ";

/// The end of a document.
const ROOT_END: &[u8] = b"</Objs>";

/// The start of a record written as text. Of a document's elements, only
/// a record's own has an `S` attribute, which names the record's stream.
const TEXT_RECORD: &[u8] = b"<S S=\"";

/// The start of a record written as an object.
const OBJECT_RECORD: &[u8] = b"<Obj S=\"";

/// The longest name an XML character reference can have, `#x10FFFF`.
const LONGEST_REFERENCE: usize = 8;

/// Turns an error stream that may contain CLIXML documents into plain
/// text, one read at a time.
pub(crate) struct Decoder {
    /// Plain bytes not yet written, as they may start a `#< CLIXML` line,
    /// or the root element of a document that is due.
    candidate: Vec<u8>,
    /// Whether the next plain byte starts a line.
    line_start: bool,
    /// `#< CLIXML` lines whose document has not started.
    due: usize,
    /// The documents started and not ended, the innermost last.
    documents: Vec<Document>,
    /// Whether the text written so far ends with a line break, as it does
    /// before any is written.
    ended_line: bool,
}

/// What has arrived of a document since the last of its records that was
/// written.
#[derive(Default)]
struct Document {
    bytes: Vec<u8>,
    /// The offsets in `bytes` at which a read started, in order.
    reads: Vec<usize>,
}

/// What the bytes of a possible `#< CLIXML` line or root start tag are.
enum Candidate {
    /// The start of one, so far.
    Partial,
    /// A whole `#< CLIXML` line.
    Marker,
    /// A whole root start tag.
    Root,
    /// Neither, so they are plain text.
    Plain,
}

/// What the start of a document's unwritten bytes is.
enum Next {
    /// A record `len` bytes long, and the line it is written as, if any.
    Record { len: usize, line: Option<Vec<u8>> },
    /// The end of the document.
    End,
    /// `len` bytes that are not a record.
    Other(usize),
    /// Not whole yet.
    Partial,
}

/// What writing a document's whole records came to.
enum Records {
    /// The document ended, before the bytes given.
    Ended(Document),
    /// Records were written.
    Wrote,
    /// None was whole.
    Waiting,
}

impl Decoder {
    pub(crate) fn new() -> Decoder {
        Decoder { candidate: Vec::new(), line_start: true, due: 0, documents: Vec::new(), ended_line: true }
    }

    /// Takes the bytes of one read of the stream and adds to `out` the
    /// text they complete.
    pub(crate) fn read(&mut self, bytes: &[u8], out: &mut Vec<u8>) {
        self.take(bytes, true, false, out);
    }

    /// Adds to `out` what is left once the stream has ended, then a line
    /// break when the text does not end with one.
    pub(crate) fn end(&mut self, out: &mut Vec<u8>) {
        self.take(&[], false, true, out);
        while let Some(document) = self.documents.pop() {
            write_rest(&mut self.ended_line, &document.bytes, out);
        }
        let candidate = std::mem::take(&mut self.candidate);
        self.write_plain(&candidate, out);
        if !self.ended_line {
            out.push(b'\n');
            self.ended_line = true;
        }
    }

    /// Takes `bytes`, the whole of a read when `new_read` is set, and adds
    /// to `out` the text they complete. `at_end` is set once the stream
    /// has ended.
    fn take(&mut self, bytes: &[u8], new_read: bool, at_end: bool, out: &mut Vec<u8>) {
        let mut after_end = Vec::new();
        let mut from_after_end = false;
        let mut starts_read = new_read;
        loop {
            let input: &[u8] = if from_after_end { &after_end } else { bytes };
            let used = if self.documents.is_empty() { self.plain(input, out) } else { 0 };
            let Some(document) = self.documents.last_mut() else { return };
            if starts_read && used == 0 {
                document.reads.push(document.bytes.len());
            }
            document.bytes.extend_from_slice(&input[used..]);
            starts_read = false;
            match self.drain(at_end, out) {
                Some(plain) => {
                    after_end = plain;
                    from_after_end = true;
                }
                None => return,
            }
        }
    }

    /// Writes the plain text at the start of `bytes` to `out` and returns
    /// how many bytes it took: all of them, or those through a root start
    /// tag, after which a document has started.
    fn plain(&mut self, bytes: &[u8], out: &mut Vec<u8>) -> usize {
        let mut at = 0;
        while at < bytes.len() {
            let byte = bytes[at];
            at += 1;
            let starts = (self.line_start && byte == b'#') || (self.due > 0 && byte == b'<');
            if self.candidate.is_empty() && !starts {
                self.write_plain(&[byte], out);
                continue;
            }
            self.candidate.push(byte);
            match candidate(&self.candidate) {
                Candidate::Partial => {}
                Candidate::Marker => {
                    self.candidate.clear();
                    self.due += 1;
                    self.line_start = true;
                }
                Candidate::Root => {
                    self.candidate.clear();
                    self.due = self.due.saturating_sub(1);
                    self.documents.push(Document::default());
                    return at;
                }
                Candidate::Plain => {
                    // The last byte may start a candidate of its own, so it
                    // is taken again after the rest is written.
                    let text = std::mem::take(&mut self.candidate);
                    let (head, last) = text.split_at(text.len() - 1);
                    if head.is_empty() {
                        self.write_plain(last, out);
                    } else {
                        self.write_plain(head, out);
                        at -= 1;
                    }
                }
            }
        }
        bytes.len()
    }

    /// Writes plain `bytes` to `out` as they are.
    fn write_plain(&mut self, bytes: &[u8], out: &mut Vec<u8>) {
        if let Some(&last) = bytes.last() {
            out.extend_from_slice(bytes);
            self.line_start = is_line_break(last);
            self.ended_line = self.line_start;
        }
    }

    /// Writes what it can of the innermost document. Returns the bytes
    /// after the end of the outermost document once that has ended, which
    /// are plain text.
    fn drain(&mut self, at_end: bool, out: &mut Vec<u8>) -> Option<Vec<u8>> {
        loop {
            let document = self.documents.last()?;
            let line_break = first_line_break(&document.bytes, at_end);
            let nested = find(&document.bytes, ROOT);
            let limit = line_break.into_iter().chain(nested).min().unwrap_or(document.bytes.len());
            match self.write_records(limit, out) {
                Records::Ended(after) => {
                    self.documents.pop();
                    match self.documents.last_mut() {
                        Some(outer) => outer.append(after),
                        None => {
                            self.line_start = true;
                            return Some(after.bytes);
                        }
                    }
                }
                Records::Wrote => {}
                Records::Waiting => match (line_break, nested) {
                    (Some(at), None) => self.write_inserted(at, out),
                    (Some(at), Some(root)) if at < root => self.write_inserted(at, out),
                    (_, Some(root)) => {
                        if !self.start_nested(root) {
                            return None;
                        }
                    }
                    (None, None) => return None,
                },
            }
        }
    }

    /// Writes the whole records at the start of the innermost document
    /// that end by `limit`, and takes them out of it.
    fn write_records(&mut self, limit: usize, out: &mut Vec<u8>) -> Records {
        let Some(document) = self.documents.last_mut() else { return Records::Waiting };
        let mut at = 0;
        let ended = loop {
            match next(&document.bytes[at..limit]) {
                Next::Record { len, line } => {
                    if let Some(line) = line {
                        write_line(&mut self.ended_line, &line, out);
                    }
                    at += len;
                }
                Next::Other(len) => {
                    write_line(&mut self.ended_line, &document.bytes[at..at + len], out);
                    at += len;
                }
                Next::End => break true,
                Next::Partial => break false,
            }
        };
        if ended {
            return Records::Ended(document.split_off(at + ROOT_END.len()));
        }
        if at == 0 {
            return Records::Waiting;
        }
        document.cut(0, at);
        Records::Wrote
    }

    /// Writes the text another writer put inside the innermost document,
    /// which ends with the line break at `at`, and takes it out of the
    /// document. A `#< CLIXML` line there is a document still to come.
    fn write_inserted(&mut self, at: usize, out: &mut Vec<u8>) {
        let Some(document) = self.documents.last_mut() else { return };
        let bytes = &document.bytes;
        let end = if bytes[at] == b'\r' && bytes.get(at + 1) == Some(&b'\n') { at + 2 } else { at + 1 };
        let marker = bytes[..at].ends_with(MARKER);
        let from = if marker {
            at - MARKER.len()
        } else {
            let tag_end = last_tag_end(&bytes[..at]);
            document.reads.iter().copied().find(|r| (tag_end..=at).contains(r)).unwrap_or(tag_end)
        };
        let line = document.cut(from, end);
        if marker {
            self.due += 1;
        } else {
            write_line(&mut self.ended_line, &line, out);
        }
    }

    /// Starts the document whose root element starts at `at` in the
    /// innermost document, once its start tag is whole, and returns
    /// whether it has.
    fn start_nested(&mut self, at: usize) -> bool {
        let Some(outer) = self.documents.last_mut() else { return false };
        let Some(close) = outer.bytes[at..].iter().position(|&b| b == b'>') else { return false };
        let inner = outer.split_off(at + close + 1);
        outer.cut(at, at + close + 1);
        self.due = self.due.saturating_sub(1);
        self.documents.push(inner);
        true
    }
}

impl Document {
    /// Takes `bytes[from..]` out of the document, with the reads that
    /// started in it.
    fn split_off(&mut self, from: usize) -> Document {
        let bytes = self.bytes.split_off(from);
        let reads = self.reads.iter().filter(|&&r| r >= from).map(|&r| r - from).collect();
        self.reads.retain(|&r| r < from);
        Document { bytes, reads }
    }

    /// Adds `other` at the end of the document.
    fn append(&mut self, other: Document) {
        let at = self.bytes.len();
        self.bytes.extend_from_slice(&other.bytes);
        self.reads.extend(other.reads.iter().map(|&r| r + at));
    }

    /// Takes `bytes[from..to]` out of the document and returns them.
    fn cut(&mut self, from: usize, to: usize) -> Vec<u8> {
        let cut: Vec<u8> = self.bytes.drain(from..to).collect();
        self.reads.retain(|&r| r < from || r >= to);
        for r in &mut self.reads {
            if *r >= to {
                *r -= to - from;
            }
        }
        cut
    }
}

/// What `bytes`, which start with `#` at the start of a line or with `<`,
/// are the start of.
fn candidate(bytes: &[u8]) -> Candidate {
    if bytes[0] == b'#' {
        let (head, line_end) = bytes.split_at(bytes.len().min(MARKER.len()));
        match line_end {
            _ if !MARKER.starts_with(head) => Candidate::Plain,
            b"" | b"\r" => Candidate::Partial,
            b"\n" | b"\r\n" => Candidate::Marker,
            _ => Candidate::Plain,
        }
    } else if bytes.len() < ROOT.len() {
        if ROOT.starts_with(bytes) { Candidate::Partial } else { Candidate::Plain }
    } else if !bytes.starts_with(ROOT) {
        Candidate::Plain
    } else {
        match bytes[ROOT.len()..].iter().position(|&b| b == b'>' || b == b'<' || is_line_break(b)) {
            None => Candidate::Partial,
            Some(at) if bytes[ROOT.len() + at] == b'>' => Candidate::Root,
            Some(_) => Candidate::Plain,
        }
    }
}

/// What a document's unwritten bytes, `bytes`, start with.
fn next(bytes: &[u8]) -> Next {
    if bytes.starts_with(ROOT_END) {
        Next::End
    } else if bytes.starts_with(TEXT_RECORD) {
        text_record(bytes)
    } else if bytes.starts_with(OBJECT_RECORD) {
        object_record(bytes)
    } else if bytes.is_empty() || may_start(bytes) {
        Next::Partial
    } else {
        match record_start(&bytes[1..]) {
            Some(at) => Next::Other(at + 1),
            None => Next::Partial,
        }
    }
}

/// Whether more bytes after `bytes` could make them the end of a document
/// or the start of a record.
fn may_start(bytes: &[u8]) -> bool {
    [ROOT_END, TEXT_RECORD, OBJECT_RECORD].iter().any(|start| start.starts_with(bytes))
}

/// The offset of the first end of a document or start of a record in
/// `bytes`.
fn record_start(bytes: &[u8]) -> Option<usize> {
    [ROOT_END, TEXT_RECORD, OBJECT_RECORD].iter().filter_map(|start| find(bytes, start)).min()
}

/// The record written as text, `<S S="stream">text</S>`, at the start of
/// `bytes`, with the line the host writes for it when it is not writing
/// CLIXML.
fn text_record(bytes: &[u8]) -> Next {
    let Some(close) = bytes.iter().position(|&b| b == b'>') else { return Next::Partial };
    let stream = stream_name(&bytes[..close]);
    if bytes[..close].ends_with(b"/") {
        return Next::Record { len: close + 1, line: Some(text_line(stream, b"")) };
    }
    let text = &bytes[close + 1..];
    let Some(end) = text.iter().position(|&b| b == b'<') else { return Next::Partial };
    let after = &text[end..];
    let len = if after.starts_with(b"</S>") {
        close + 1 + end + 4
    } else if b"</S>".starts_with(after) {
        return Next::Partial;
    } else {
        close + 1 + end
    };
    Next::Record { len, line: Some(text_line(stream, &text[..end])) }
}

/// The record written as an object, `<Obj S="stream" ...>...</Obj>`, at
/// the start of `bytes`. A progress or information record is written as
/// nothing, and another as the text of its own `ToString`, or as it is
/// when it has none.
fn object_record(bytes: &[u8]) -> Next {
    let Some(close) = bytes.iter().position(|&b| b == b'>') else { return Next::Partial };
    let shown = shown(stream_name(&bytes[..close]));
    let mut depth = usize::from(!bytes[..close].ends_with(b"/"));
    let mut text: Option<&[u8]> = None;
    let mut at = close + 1;
    let len = loop {
        if depth == 0 {
            break at;
        }
        let Some(open) = bytes[at..].iter().position(|&b| b == b'<').map(|o| at + o) else { return Next::Partial };
        let rest = &bytes[open..];
        if rest.starts_with(ROOT_END) || rest.starts_with(TEXT_RECORD) || rest.starts_with(OBJECT_RECORD) {
            // The record's own end never came.
            break open;
        }
        if may_start(rest) {
            return Next::Partial;
        }
        let Some(gt) = rest.iter().position(|&b| b == b'>') else { return Next::Partial };
        let tag = &rest[..=gt];
        at = open + gt + 1;
        if tag_name(tag) == b"Obj" {
            if tag.starts_with(b"</") {
                depth -= 1;
            } else if !tag.ends_with(b"/>") {
                depth += 1;
            }
        } else if depth == 1 && tag == b"<ToString>" && text.is_none() {
            let Some(end) = bytes[at..].iter().position(|&b| b == b'<') else { return Next::Partial };
            text = Some(&bytes[at..at + end]);
        }
    };
    let line = shown.then(|| match text {
        Some(text) => {
            let mut line = Vec::new();
            unescape(text, &mut line);
            line
        }
        None => bytes[..len].to_vec(),
    });
    Next::Record { len, line }
}

/// Whether a record of `stream` is written: progress and information
/// records are not.
fn shown(stream: &[u8]) -> bool {
    !stream.eq_ignore_ascii_case(b"progress") && !stream.eq_ignore_ascii_case(b"information")
}

/// The value of the `S` attribute in `tag`, a start tag less its `>`.
fn stream_name(tag: &[u8]) -> &[u8] {
    let Some(at) = find(tag, b" S=\"") else { return b"" };
    let value = &tag[at + 4..];
    let end = value.iter().position(|&b| b == b'"').unwrap_or(value.len());
    &value[..end]
}

/// The element name in `tag`.
fn tag_name(tag: &[u8]) -> &[u8] {
    let inner = tag.strip_prefix(b"</").or_else(|| tag.strip_prefix(b"<")).unwrap_or(tag);
    let len = inner.iter().take_while(|b| b.is_ascii_alphanumeric()).count();
    &inner[..len]
}

/// The line the host writes, when it is not writing CLIXML, for a record
/// of `stream` whose text is `text`.
fn text_line(stream: &[u8], text: &[u8]) -> Vec<u8> {
    let prefix: &[u8] = if stream.eq_ignore_ascii_case(b"warning") {
        b"WARNING: "
    } else if stream.eq_ignore_ascii_case(b"verbose") {
        b"VERBOSE: "
    } else if stream.eq_ignore_ascii_case(b"debug") {
        b"DEBUG: "
    } else {
        b""
    };
    let mut line = prefix.to_vec();
    unescape(text, &mut line);
    line
}

/// Writes what is left of a document once the stream has ended: the text
/// of a text record whose end never came, nothing for a progress or
/// information record, and anything else as it is.
fn write_rest(ended_line: &mut bool, bytes: &[u8], out: &mut Vec<u8>) {
    if bytes.is_empty() {
        return;
    }
    match bytes.iter().position(|&b| b == b'>') {
        Some(close) if bytes.starts_with(TEXT_RECORD) => {
            let text = &bytes[close + 1..];
            let end = text.iter().position(|&b| b == b'<').unwrap_or(text.len());
            write_line(ended_line, &text_line(stream_name(&bytes[..close]), &text[..end]), out);
        }
        Some(close) if bytes.starts_with(OBJECT_RECORD) && !shown(stream_name(&bytes[..close])) => {}
        _ => write_line(ended_line, bytes, out),
    }
}

/// Writes `line` to `out` on a line of its own, with a line break after
/// it when it ends without one.
fn write_line(ended_line: &mut bool, line: &[u8], out: &mut Vec<u8>) {
    if !*ended_line {
        out.push(b'\n');
    }
    out.extend_from_slice(line);
    if !line.last().is_some_and(|&b| is_line_break(b)) {
        out.push(b'\n');
    }
    *ended_line = true;
}

/// The offset of the first line break in `bytes`, unless it is a carriage
/// return at their end that a line feed may follow, the stream not yet
/// having ended.
fn first_line_break(bytes: &[u8], at_end: bool) -> Option<usize> {
    let at = bytes.iter().position(|&b| is_line_break(b))?;
    (at_end || bytes[at] == b'\n' || at + 1 < bytes.len()).then_some(at)
}

/// The offset just past the last whole tag in `bytes`, or 0 when there is
/// none.
fn last_tag_end(bytes: &[u8]) -> usize {
    let mut end = bytes.len();
    while let Some(close) = bytes[..end].iter().rposition(|&b| b == b'>') {
        if let Some(open) = bytes[..close].iter().rposition(|&b| b == b'<')
            && is_tag(&bytes[open..=close])
        {
            return close + 1;
        }
        end = close;
    }
    0
}

/// Whether `tag`, from `<` through `>`, has the shape of a tag CLIXML
/// writes: `</name>`, or `<name` with any ` key="value"` after it and
/// then `>` or ` />`.
fn is_tag(tag: &[u8]) -> bool {
    let Some(inner) = tag.strip_prefix(b"<").and_then(|t| t.strip_suffix(b">")) else { return false };
    let (inner, end_tag) = match inner.strip_prefix(b"/") {
        Some(rest) => (rest, true),
        None => (inner, false),
    };
    let name = inner.iter().take_while(|b| b.is_ascii_alphanumeric()).count();
    if name == 0 || !inner[0].is_ascii_alphabetic() {
        return false;
    }
    let mut rest = &inner[name..];
    if end_tag {
        return rest.is_empty();
    }
    loop {
        if rest.is_empty() || rest == b" /" {
            return true;
        }
        let Some(attribute) = rest.strip_prefix(b" ") else { return false };
        let key = attribute.iter().take_while(|b| b.is_ascii_alphabetic()).count();
        let Some(value) = attribute[key..].strip_prefix(b"=\"") else { return false };
        let Some(quote) = value.iter().position(|&b| b == b'"') else { return false };
        if key == 0 {
            return false;
        }
        rest = &value[quote + 1..];
    }
}

/// Adds `text` to `out` with CLIXML's escapes undone: XML's entity and
/// character references, and the `_xHHHH_` PowerShell writes for a UTF-16
/// code unit that XML cannot have as it is. Every other byte is added as
/// it is.
fn unescape(text: &[u8], out: &mut Vec<u8>) {
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        if let Some((unit, len)) = code_unit(rest) {
            if (0xD800..0xDC00).contains(&unit)
                && let Some((low, more)) = code_unit(&rest[len..])
                && (0xDC00..0xE000).contains(&low)
            {
                push_char(0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00), out);
                at += len + more;
            } else {
                push_char(unit, out);
                at += len;
            }
        } else if let Some((c, len)) = reference(rest) {
            push_char(c, out);
            at += len;
        } else {
            out.push(text[at]);
            at += 1;
        }
    }
}

/// The UTF-16 code unit an `_xHHHH_` at the start of `text` stands for,
/// and its length.
fn code_unit(text: &[u8]) -> Option<(u32, usize)> {
    let escape = text.strip_prefix(b"_x")?.get(..5)?;
    if escape[4] != b'_' {
        return None;
    }
    Some((number(&escape[..4], 16)?, 7))
}

/// The character an XML reference at the start of `text` stands for, and
/// its length.
fn reference(text: &[u8]) -> Option<(u32, usize)> {
    let rest = text.strip_prefix(b"&")?;
    let semicolon = rest.iter().take(LONGEST_REFERENCE + 1).position(|&b| b == b';')?;
    let name = &rest[..semicolon];
    let c = match name {
        b"lt" => u32::from('<'),
        b"gt" => u32::from('>'),
        b"amp" => u32::from('&'),
        b"quot" => u32::from('"'),
        b"apos" => u32::from('\''),
        _ => {
            let digits = name.strip_prefix(b"#")?;
            match digits.strip_prefix(b"x").or_else(|| digits.strip_prefix(b"X")) {
                Some(hex) => number(hex, 16)?,
                None => number(digits, 10)?,
            }
        }
    };
    Some((c, semicolon + 2))
}

/// The value of `digits` in `radix`, when there is at least one and every
/// one is a digit of it. No caller passes more than seven, and no seven
/// digits overflow.
fn number(digits: &[u8], radix: u32) -> Option<u32> {
    if digits.is_empty() {
        return None;
    }
    let mut value = 0;
    for &digit in digits {
        value = value * radix + char::from(digit).to_digit(radix)?;
    }
    Some(value)
}

/// Adds the code point `c` to `out` in UTF-8, or U+FFFD for one that is
/// not a character, such as a lone surrogate.
fn push_char(c: u32, out: &mut Vec<u8>) {
    let c = char::from_u32(c).unwrap_or(char::REPLACEMENT_CHARACTER);
    out.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
}

fn is_line_break(b: u8) -> bool {
    b == b'\r' || b == b'\n'
}

/// The offset of the first `needle` in `haystack`.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::Decoder;

    /// The `#< CLIXML` line, as a host writes it on Windows.
    const MARKER_LINE: &[u8] = b"#< CLIXML\r\n";

    /// The root start tag both hosts write.
    const ROOT: &[u8] = b"<Objs Version=\"1.1.0.1\" xmlns=\"http://schemas.microsoft.com/powershell/2004/04\">";

    const END: &[u8] = b"</Objs>";

    /// The progress record Windows PowerShell writes as it prepares its
    /// modules.
    const PREPARING: &[u8] = b"<Obj S=\"progress\" RefId=\"0\"><TN RefId=\"0\"><T>System.Management.Automation.PSCustomObject</T><T>System.Object</T></TN><MS><I64 N=\"SourceId\">1</I64><PR N=\"Record\"><AV>Preparing modules for first use.</AV><AI>0</AI><Nil /><PI>-1</PI><PC>-1</PC><T>Completed</T><SR>-1</SR><SD> </SD></PR></MS></Obj>";

    /// The error stream of a child Windows PowerShell started with
    /// -EncodedCommand, which wrote a record of each kind and a line
    /// straight to the stream, as captured but for the user and computer
    /// names and the times in its information records. Its error repeats
    /// the script, which -EncodedCommand makes the command line.
    const WINDOWS_POWERSHELL: &[&[u8]] = &[
        b"#< CLIXML\r\nstraight to stderr\r\n",
        ROOT,
        PREPARING,
        b"<S S=\"Error\">$ErrorActionPreference = 'Continue'_x000D__x000A_</S>",
        b"<S S=\"Error\">$VerbosePreference = 'Continue'_x000D__x000A_</S>",
        b"<S S=\"Error\">$DebugPreference = 'Continue'_x000D__x000A_</S>",
        b"<S S=\"Error\">$InformationPreference = 'Continue'_x000D__x000A_</S>",
        b"<S S=\"Error\">'out: first'_x000D__x000A_</S>",
        b"<S S=\"Error\">Write-Error 'an error'_x000D__x000A_</S>",
        b"<S S=\"Error\">Write-Warning 'a warning'_x000D__x000A_</S>",
        b"<S S=\"Error\">Write-Verbose 'a verbose line'_x000D__x000A_</S>",
        b"<S S=\"Error\">Write-Debug 'a debug line'_x000D__x000A_</S>",
        b"<S S=\"Error\">Write-Information 'an information record'_x000D__x000A_</S>",
        b"<S S=\"Error\">Write-Host 'a Write-Host line'_x000D__x000A_</S>",
        b"<S S=\"Error\">Write-Progress -Activity 'probing' -Status 'half way' -PercentComplete 50_x000D__x000A_</S>",
        b"<S S=\"Error\">Write-Warning \"tab`there, escape $([char]27)[1m bold, under_x005F_x0041_score, emoji $([char]::ConvertFromUtf32(0x1F600)), _x000D__x000A_</S>",
        b"<S S=\"Error\">nul $([char]0) end\"_x000D__x000A_</S>",
        b"<S S=\"Error\">[Console]::Error.WriteLine('straight to stderr')_x000D__x000A_</S>",
        b"<S S=\"Error\">'out: last'_x000D__x000A_</S>",
        b"<S S=\"Error\">exit 3_x000D__x000A_</S>",
        b"<S S=\"Error\"> : an error_x000D__x000A_</S>",
        b"<S S=\"Error\">    + CategoryInfo          : NotSpecified: (:) [Write-Error], WriteErrorException_x000D__x000A_</S>",
        b"<S S=\"Error\">    + FullyQualifiedErrorId : Microsoft.PowerShell.Commands.WriteErrorException_x000D__x000A_</S>",
        b"<S S=\"Error\"> _x000D__x000A_</S>",
        b"<S S=\"warning\">a warning</S>",
        b"<S S=\"verbose\">a verbose line</S>",
        b"<S S=\"debug\">a debug line</S>",
        b"<Obj S=\"information\" RefId=\"1\"><TN RefId=\"1\"><T>System.Management.Automation.InformationRecord</T><T>System.Object</T></TN><ToString>an information record</ToString><Props><S N=\"MessageData\">an information record</S><S N=\"Source\">Write-Information</S><DT N=\"TimeGenerated\">0001-01-01T00:00:00</DT><Obj N=\"Tags\" RefId=\"2\"><TN RefId=\"2\"><T>System.Collections.Generic.List`1[[System.String, mscorlib, Version=4.0.0.0, Culture=neutral, PublicKeyToken=b77a5c561934e089]]</T><T>System.Object</T></TN><LST /></Obj><S N=\"User\">HOST\\user</S><S N=\"Computer\">HOST</S><U32 N=\"ProcessId\">22524</U32><U32 N=\"NativeThreadId\">70956</U32><U32 N=\"ManagedThreadId\">7</U32></Props></Obj>",
        b"<Obj S=\"information\" RefId=\"3\"><TNRef RefId=\"1\" /><ToString>a Write-Host line</ToString><Props><Obj N=\"MessageData\" RefId=\"4\"><TN RefId=\"3\"><T>System.Management.Automation.HostInformationMessage</T><T>System.Object</T></TN><ToString>a Write-Host line</ToString><Props><S N=\"Message\">a Write-Host line</S><B N=\"NoNewLine\">false</B><S N=\"ForegroundColor\">Gray</S><S N=\"BackgroundColor\">Black</S></Props></Obj><S N=\"Source\">Write-Host</S><DT N=\"TimeGenerated\">0001-01-01T00:00:00</DT><Obj N=\"Tags\" RefId=\"5\"><TNRef RefId=\"2\" /><LST><S>PSHOST</S></LST></Obj><S N=\"User\">HOST\\user</S><S N=\"Computer\">HOST</S><U32 N=\"ProcessId\">22524</U32><U32 N=\"NativeThreadId\">70956</U32><U32 N=\"ManagedThreadId\">7</U32></Props></Obj>",
        b"<Obj S=\"progress\" RefId=\"6\"><TNRef RefId=\"0\" /><MS><I64 N=\"SourceId\">0</I64><PR N=\"Record\"><AV>probing</AV><AI>0</AI><Nil /><PI>-1</PI><PC>50</PC><T>Processing</T><SR>-1</SR><SD>half way</SD></PR></MS></Obj>",
        b"<S S=\"warning\">tab_x0009_here, escape _x001B_[1m bold, under_x005F_x0041_score, emoji _xD83D__xDE00_, nul _x0000_ end</S>",
        END,
    ];

    /// What [`WINDOWS_POWERSHELL`] is written as.
    const WINDOWS_POWERSHELL_TEXT: &[&[u8]] = &[
        b"straight to stderr\r\n",
        b"$ErrorActionPreference = 'Continue'\r\n",
        b"$VerbosePreference = 'Continue'\r\n",
        b"$DebugPreference = 'Continue'\r\n",
        b"$InformationPreference = 'Continue'\r\n",
        b"'out: first'\r\n",
        b"Write-Error 'an error'\r\n",
        b"Write-Warning 'a warning'\r\n",
        b"Write-Verbose 'a verbose line'\r\n",
        b"Write-Debug 'a debug line'\r\n",
        b"Write-Information 'an information record'\r\n",
        b"Write-Host 'a Write-Host line'\r\n",
        b"Write-Progress -Activity 'probing' -Status 'half way' -PercentComplete 50\r\n",
        b"Write-Warning \"tab`there, escape $([char]27)[1m bold, under_x0041_score, emoji $([char]::ConvertFromUtf32(0x1F600)), \r\n",
        b"nul $([char]0) end\"\r\n",
        b"[Console]::Error.WriteLine('straight to stderr')\r\n",
        b"'out: last'\r\n",
        b"exit 3\r\n",
        b" : an error\r\n",
        b"    + CategoryInfo          : NotSpecified: (:) [Write-Error], WriteErrorException\r\n",
        b"    + FullyQualifiedErrorId : Microsoft.PowerShell.Commands.WriteErrorException\r\n",
        b" \r\n",
        b"WARNING: a warning\n",
        b"VERBOSE: a verbose line\n",
        b"DEBUG: a debug line\n",
        b"WARNING: tab\there, escape \x1b[1m bold, under_x0041_score, emoji \xf0\x9f\x98\x80, nul \x00 end\n",
    ];

    /// The same script's error stream from pwsh, captured as
    /// [`WINDOWS_POWERSHELL`] was. Its error has the colors pwsh gives it.
    const PWSH: &[&[u8]] = &[
        b"#< CLIXML\r\nstraight to stderr\r\n",
        ROOT,
        b"<S S=\"Error\">_x001B_[31;1mWrite-Error: _x001B_[31;1man error_x001B_[0m_x000D__x000A_</S>",
        b"<S S=\"warning\">a warning</S>",
        b"<S S=\"verbose\">a verbose line</S>",
        b"<S S=\"debug\">a debug line</S>",
        b"<Obj S=\"information\" RefId=\"0\"><TN RefId=\"0\"><T>System.Management.Automation.InformationRecord</T><T>System.Object</T></TN><ToString>an information record</ToString><Props><S N=\"MessageData\">an information record</S><S N=\"Source\">Write-Information</S><DT N=\"TimeGenerated\">0001-01-01T00:00:00</DT><Obj N=\"Tags\" RefId=\"1\"><TN RefId=\"1\"><T>System.Collections.Generic.List`1[[System.String, System.Private.CoreLib, Version=10.0.0.0, Culture=neutral, PublicKeyToken=7cec85d7bea7798e]]</T><T>System.Object</T></TN><LST /></Obj><S N=\"User\">HOST\\user</S><S N=\"Computer\">HOST</S><U32 N=\"ProcessId\">71712</U32><U32 N=\"NativeThreadId\">69868</U32><U32 N=\"ManagedThreadId\">17</U32></Props></Obj>",
        b"<Obj S=\"information\" RefId=\"2\"><TNRef RefId=\"0\" /><ToString>a Write-Host line</ToString><Props><Obj N=\"MessageData\" RefId=\"3\"><TN RefId=\"2\"><T>System.Management.Automation.HostInformationMessage</T><T>System.Object</T></TN><ToString>a Write-Host line</ToString><Props><S N=\"Message\">a Write-Host line</S><B N=\"NoNewLine\">false</B><S N=\"ForegroundColor\">Gray</S><S N=\"BackgroundColor\">Black</S></Props></Obj><S N=\"Source\">Write-Host</S><DT N=\"TimeGenerated\">0001-01-01T00:00:00</DT><Obj N=\"Tags\" RefId=\"4\"><TNRef RefId=\"1\" /><LST><S>PSHOST</S></LST></Obj><S N=\"User\">HOST\\user</S><S N=\"Computer\">HOST</S><U32 N=\"ProcessId\">71712</U32><U32 N=\"NativeThreadId\">69868</U32><U32 N=\"ManagedThreadId\">17</U32></Props></Obj>",
        b"<Obj S=\"progress\" RefId=\"5\"><TN RefId=\"3\"><T>System.Management.Automation.PSCustomObject</T><T>System.Object</T></TN><MS><I64 N=\"SourceId\">0</I64><PR N=\"Record\"><AV>probing</AV><AI>0</AI><Nil /><PI>-1</PI><PC>50</PC><T>Processing</T><SR>-1</SR><SD>half way</SD></PR></MS></Obj>",
        b"<S S=\"warning\">tab_x0009_here, escape _x001B_[1m bold, under_x005F_x0041_score, emoji _xD83D__xDE00_, nul _x0000_ end</S>",
        END,
    ];

    /// The error stream of a child pwsh on Linux started with
    /// -EncodedCommand, which wrote a warning, a progress record and a
    /// line straight to the stream, as captured. Its `#< CLIXML` line ends
    /// with a line feed alone.
    const LINUX_PWSH: &[&[u8]] = &[
        b"#< CLIXML\na plain line\n",
        ROOT,
        b"<S S=\"warning\">a warning</S>",
        b"<Obj S=\"progress\" RefId=\"0\"><TN RefId=\"0\"><T>System.Management.Automation.PSCustomObject</T><T>System.Object</T></TN><MS><I64 N=\"SourceId\">0</I64><PR N=\"Record\"><AV>work</AV><AI>0</AI><Nil /><PI>-1</PI><PC>50</PC><T>Processing</T><SR>-1</SR><SD>half way</SD></PR></MS></Obj>",
        END,
    ];

    /// What [`LINUX_PWSH`] is written as.
    const LINUX_PWSH_TEXT: &[&[u8]] = &[b"a plain line\n", b"WARNING: a warning\n"];

    /// What [`PWSH`] is written as.
    const PWSH_TEXT: &[&[u8]] = &[
        b"straight to stderr\r\n",
        b"\x1b[31;1mWrite-Error: \x1b[31;1man error\x1b[0m\r\n",
        b"WARNING: a warning\n",
        b"VERBOSE: a verbose line\n",
        b"DEBUG: a debug line\n",
        b"WARNING: tab\there, escape \x1b[1m bold, under_x0041_score, emoji \xf0\x9f\x98\x80, nul \x00 end\n",
    ];

    /// Records from documents of more than 6 KB, from both hosts, each cut
    /// where the host's write of one piece of its document ended and a
    /// line the host wrote straight to the stream landed: in a text, after
    /// a tag, in a tag's name, after a bare `<`, in an end tag and in an
    /// attribute's value. Each is the part before the line, the line, and
    /// the part after it, as captured.
    const CUT: &[(&[u8], &[u8], &[u8])] = &[
        (
            b"<Obj S=\"progress\" RefId=\"28\"><TNRef RefId=\"0\" /><MS><I64 N=\"SourceId\">0</I64><PR N=\"Record\"><AV>chunked</AV><AI>0</AI><Nil /><PI>-1</PI><PC>28</PC><T>Pro",
            b"plain line after record 50\r\n",
            b"cessing</T><SR>-1</SR><SD>record 28</SD></PR></MS></Obj>",
        ),
        (
            b"<Obj S=\"progress\" RefId=\"87\"><TNRef RefId=\"0\" /><MS><I64 N=\"SourceId\">",
            b"plain line after record 100\r\n",
            b"0</I64><PR N=\"Record\"><AV>chunked</AV><AI>0</AI><Nil /><PI>-1</PI><PC>87</PC><T>Processing</T><SR>-1</SR><SD>record 87</SD></PR></MS></Obj>",
        ),
        (
            b"<Obj S=\"progress\" RefId=\"145\"><TNRef RefId=\"0\" /><MS><I64 N=\"SourceId\">0</I64><PR N=\"Record\"><AV>chunked</AV><A",
            b"plain line after record 150\r\n",
            b"I>0</AI><Nil /><PI>-1</PI><PC>45</PC><T>Processing</T><SR>-1</SR><SD>record 145</SD></PR></MS></Obj>",
        ),
        (
            b"<Obj S=\"progress\" RefId=\"174\"><TNRef RefId=\"0\" /><MS><I64 N=\"SourceId\">0</I64><PR N=\"Record\"><",
            b"plain line after record 200\r\n",
            b"AV>chunked</AV><AI>0</AI><Nil /><PI>-1</PI><PC>74</PC><T>Processing</T><SR>-1</SR><SD>record 174</SD></PR></MS></Obj>",
        ),
        (
            b"<Obj S=\"progress\" RefId=\"348\"><TNRef RefId=\"0\" /><MS><I64 N=\"SourceId\">0</I64><PR N=\"Record\"><AV>chunked</AV><AI>0</AI><Nil /><PI>-1</P",
            b"plain line after record 350\r\n",
            b"I><PC>48</PC><T>Processing</T><SR>-1</SR><SD>record 348</SD></PR></MS></Obj>",
        ),
        (
            b"<Obj S=\"progress\" RefId=\"377\"><TNRef RefId=\"0\" /><MS><I64 N=\"SourceId\">0</I64><PR N=\"Record\"><AV>chunked</AV><AI>0</",
            b"plain line after record 400\r\n",
            b"AI><Nil /><PI>-1</PI><PC>77</PC><T>Processing</T><SR>-1</SR><SD>record 377</SD></PR></MS></Obj>",
        ),
        (
            b"<Obj S=\"progress\" RefId=\"87\"><TNRef RefId=\"0\" /><MS><I64 N=\"SourceId\">0</I64><PR N=\"",
            b"plain line after record 100\r\n",
            b"Record\"><AV>chunked</AV><AI>0</AI><Nil /><PI>-1</PI><PC>88</PC><T>Processing</T><SR>-1</SR><SD>record 88</SD></PR></MS></Obj>",
        ),
    ];

    /// What each line of [`CUT`] is written as when one read brings it
    /// with the part of the record before it: from the end of the last
    /// whole tag.
    const CUT_IN_ONE_READ: &[&[u8]] = &[
        b"Proplain line after record 50\r\n",
        b"plain line after record 100\r\n",
        b"<Aplain line after record 150\r\n",
        b"<plain line after record 200\r\n",
        b"-1</Pplain line after record 350\r\n",
        b"0</plain line after record 400\r\n",
        b"<PR N=\"plain line after record 100\r\n",
    ];

    /// `reads` decoded, one after another.
    fn decoded(reads: &[&[u8]]) -> Vec<u8> {
        let mut decoder = Decoder::new();
        let mut out = Vec::new();
        for read in reads {
            decoder.read(read, &mut out);
        }
        decoder.end(&mut out);
        out
    }

    /// `bytes` decoded a byte to a read.
    fn decoded_bytewise(bytes: &[u8]) -> Vec<u8> {
        let reads: Vec<&[u8]> = bytes.chunks(1).collect();
        decoded(&reads)
    }

    /// hello's suite in Windows PowerShell: two child hosts, each started
    /// with -EncodedCommand, wrote one progress record each to the suite's
    /// error stream, as captured. None of it is written, however the reads
    /// divide it.
    #[test]
    fn a_suites_progress_records_are_left_out() {
        let stream = [MARKER_LINE, ROOT, PREPARING, END, MARKER_LINE, ROOT, PREPARING, END].concat();
        assert_eq!(decoded(&[&stream]), b"");
        assert_eq!(decoded_bytewise(&stream), b"");
    }

    /// A line that follows a document on the same line, as cargo-pwrs's
    /// surface line followed the suite's documents in a gate's captured
    /// log, is written whole.
    #[test]
    fn a_line_after_a_document_on_its_line_is_written_whole() {
        let line: &[u8] = b"pwrs: surface check: every cmdlet ran, every pipeline parameter took a record, every ShouldProcess cmdlet called should_process\n";
        let stream = [MARKER_LINE, ROOT, PREPARING, END, MARKER_LINE, ROOT, PREPARING, END, line].concat();
        assert_eq!(decoded(&[&stream]), line);
    }

    /// Each kind of record is written as the host writes it when it is not
    /// writing CLIXML, in both hosts and on Linux, however the reads divide
    /// the stream.
    #[test]
    fn each_record_is_written_as_the_host_writes_it_as_text() {
        for (stream, text) in [(WINDOWS_POWERSHELL, WINDOWS_POWERSHELL_TEXT), (PWSH, PWSH_TEXT), (LINUX_PWSH, LINUX_PWSH_TEXT)] {
            let stream = stream.concat();
            let text = text.concat();
            assert_eq!(String::from_utf8_lossy(&decoded(&[&stream])), String::from_utf8_lossy(&text));
            assert_eq!(decoded_bytewise(&stream), text);
        }
    }

    /// A document with lines cut into it, its reads divided as the hosts'
    /// were, each line a read of its own: every line is written whole,
    /// where it landed, and the records around them are left out as the
    /// progress records they are. A line read with the part of the record
    /// after it, or in two reads, starts where its first read did.
    #[test]
    fn a_line_inside_a_document_is_written_from_where_its_read_began() {
        let lines: Vec<u8> = CUT.iter().flat_map(|(_, line, _)| line.iter().copied()).collect();
        let start = [MARKER_LINE, ROOT, PREPARING].concat();
        let mut alone = vec![start.clone()];
        let mut with_after = vec![start.clone()];
        let mut halves = vec![start];
        for (before, line, after) in CUT {
            alone.extend([before.to_vec(), line.to_vec(), after.to_vec()]);
            with_after.extend([before.to_vec(), [*line, *after].concat()]);
            let (first, second) = line.split_at(line.len() / 2);
            halves.extend([before.to_vec(), first.to_vec(), [second, *after].concat()]);
        }
        for mut reads in [alone, with_after, halves] {
            reads.push(END.to_vec());
            let reads: Vec<&[u8]> = reads.iter().map(Vec::as_slice).collect();
            assert_eq!(String::from_utf8_lossy(&decoded(&reads)), String::from_utf8_lossy(&lines));
        }
    }

    /// The same document in one read: each line is written from the end of
    /// the last whole tag before it, with the part of a tag or a text that
    /// came before it in front, and nothing of it is lost.
    #[test]
    fn a_line_inside_a_document_in_one_read_is_written_from_the_last_whole_tag() {
        let mut stream = [MARKER_LINE, ROOT, PREPARING].concat();
        for (before, line, after) in CUT {
            stream.extend([*before, *line, *after].concat());
        }
        stream.extend_from_slice(END);
        assert_eq!(String::from_utf8_lossy(&decoded(&[&stream])), String::from_utf8_lossy(&CUT_IN_ONE_READ.concat()));
    }

    /// A child's document inside another child's, its `#< CLIXML` line
    /// with it, is read on its own, and the record it landed in is read
    /// whole around it.
    #[test]
    fn a_document_inside_a_document_is_read_on_its_own() {
        let (before, after) = PREPARING.split_at(120);
        let inner: &[u8] = b"<S S=\"warning\">from the inner child</S>";
        let outer: &[u8] = b"<S S=\"warning\">from the outer child</S>";
        let stream = [MARKER_LINE, ROOT, before, MARKER_LINE, ROOT, inner, END, after, outer, END].concat();
        let text: &[u8] = b"WARNING: from the inner child\nWARNING: from the outer child\n";
        assert_eq!(String::from_utf8_lossy(&decoded(&[&stream])), String::from_utf8_lossy(text));
        assert_eq!(decoded_bytewise(&stream), text);
    }

    /// Plain text passes through byte for byte, however the reads divide
    /// it: a byte that is not UTF-8, a line starting with `#`, a
    /// `#< CLIXML` inside a line, a `<Objs ` with no `#< CLIXML` line
    /// before it, and a carriage return alone.
    #[test]
    fn plain_text_passes_through_byte_for_byte() {
        let text: &[u8] = b"caf\x82\r\n# a comment\r\nin a line #< CLIXML\r\n<Objs Version=\"1.1.0.1\">\r\nhalf way\r#<\n";
        assert_eq!(decoded(&[text]), text);
        assert_eq!(decoded_bytewise(text), text);
    }

    /// A stream that ends inside a document: the text of a text record
    /// whose end never came is written, a progress record is not, and the
    /// start of a `#< CLIXML` line is plain text, its line ended.
    #[test]
    fn a_stream_that_ends_inside_a_document() {
        assert_eq!(decoded(&[&[MARKER_LINE, ROOT, b"<S S=\"Error\">an err"].concat()]), b"an err\n");
        assert_eq!(decoded(&[&[MARKER_LINE, ROOT, &PREPARING[..60]].concat()]), b"");
        assert_eq!(decoded(&[b"#< CLI"]), b"#< CLI\n");
    }

    /// A record of a kind neither host writes as an object today is written
    /// as the text of its own `ToString`.
    #[test]
    fn another_kind_of_object_record_is_written_as_its_text() {
        let record: &[u8] = b"<Obj S=\"output\" RefId=\"0\"><TN RefId=\"0\"><T>System.String</T></TN><ToString>a &lt;value&gt; &#233;</ToString></Obj>";
        assert_eq!(decoded(&[&[MARKER_LINE, ROOT, record, END].concat()]), "a <value> \u{e9}\n".as_bytes());
    }
}
