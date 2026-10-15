//Vmynt
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//A reader for PowerShell data files (blueprint.psd1). Blueprints stay in this
//format while the PowerShell tool still reads them; it is a small, regular
//language, so a hand-written reader covers it:
//
//  @{ Key = value; Other = value }     a table (newlines also separate entries)
//  @( 'a', 'b' )                       a list (newlines also separate items)
//  'text'  "text"  @' ... '@           strings, single, double and here-strings
//  12  1.5  -3  4GB                    numbers, with KB/MB/GB/TB suffixes
//  $true  $false  $null
//  # comment   <# block comment #>
//
//The result is a YAML value, so blueprints merge with config using the same code.

use anyhow::{Result, bail};
use serde_norway::{Mapping, Number, Value};

//Parses a whole .psd1 file. The top level must be a table.
pub fn parse(text: &str) -> Result<Mapping> {
    let mut parser = Parser {
        chars: text.chars().collect(),
        position: 0,
    };
    parser.skip_space(true)?;
    let value = parser.value()?;
    parser.skip_space(true)?;
    if parser.position < parser.chars.len() {
        bail!(
            "line {}: unexpected text after the closing brace",
            parser.line()
        );
    }
    match value {
        Value::Mapping(map) => Ok(map),
        _ => bail!("a .psd1 file must hold one @{{ ... }} table"),
    }
}

struct Parser {
    chars: Vec<char>,
    position: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.position).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.position + offset).copied()
    }

    fn line(&self) -> usize {
        self.chars[..self.position.min(self.chars.len())]
            .iter()
            .filter(|c| **c == '\n')
            .count()
            + 1
    }

    //Skips spaces and comments. Newlines too, when `newlines` is set; tables and
    //lists use newlines as separators, so they sometimes need to see them.
    fn skip_space(&mut self, newlines: bool) -> Result<()> {
        while let Some(c) = self.peek() {
            if c == '\n' && !newlines {
                break;
            }
            if c.is_whitespace() || c == '\u{feff}' {
                self.position += 1;
            } else if c == '<' && self.peek_at(1) == Some('#') {
                let start = self.line();
                self.position += 2;
                loop {
                    match self.peek() {
                        None => bail!("line {start}: a <# comment is never closed with #>"),
                        Some('#') if self.peek_at(1) == Some('>') => {
                            self.position += 2;
                            break;
                        }
                        Some(_) => self.position += 1,
                    }
                }
            } else if c == '#' {
                while let Some(c) = self.peek() {
                    if c == '\n' {
                        break;
                    }
                    self.position += 1;
                }
            } else {
                break;
            }
        }
        Ok(())
    }

    //One value. A comma after it makes a list, as in PowerShell: 'a', 'b'.
    fn value(&mut self) -> Result<Value> {
        let first = self.single_value()?;
        self.skip_space(false)?;
        if self.peek() != Some(',') {
            return Ok(first);
        }
        let mut items = vec![first];
        while self.peek() == Some(',') {
            self.position += 1;
            self.skip_space(true)?;
            items.push(self.single_value()?);
            self.skip_space(false)?;
        }
        Ok(Value::Sequence(items))
    }

    fn single_value(&mut self) -> Result<Value> {
        match (self.peek(), self.peek_at(1)) {
            (Some('@'), Some('{')) => self.table(),
            (Some('@'), Some('(')) => self.list(),
            (Some('@'), Some('\'' | '"')) => self.here_string().map(Value::String),
            (Some('\''), _) => self.single_quoted().map(Value::String),
            (Some('"'), _) => self.double_quoted().map(Value::String),
            (Some('$'), _) => self.variable(),
            (Some(c), _) if c.is_ascii_digit() || c == '-' || c == '.' => self.number(),
            (Some(c), _) => bail!("line {}: unexpected '{c}'", self.line()),
            (None, _) => bail!("the file ends where a value was expected"),
        }
    }

    fn table(&mut self) -> Result<Value> {
        let start = self.line();
        self.position += 2;
        let mut map = Mapping::new();
        loop {
            self.skip_space(true)?;
            match self.peek() {
                None => bail!("line {start}: a @{{ table is never closed with }}"),
                Some('}') => {
                    self.position += 1;
                    return Ok(Value::Mapping(map));
                }
                Some(';') => {
                    self.position += 1;
                    continue;
                }
                Some(_) => {}
            }
            let key = self.key()?;
            self.skip_space(false)?;
            if self.peek() != Some('=') {
                bail!("line {}: expected '=' after '{key}'", self.line());
            }
            self.position += 1;
            self.skip_space(true)?;
            let value = self.value()?;
            map.insert(Value::String(key), value);
        }
    }

    fn key(&mut self) -> Result<String> {
        match self.peek() {
            Some('\'') => self.single_quoted(),
            Some('"') => self.double_quoted(),
            _ => {
                let start = self.position;
                while let Some(c) = self.peek() {
                    if c.is_alphanumeric() || c == '_' || c == '-' || c == '.' {
                        self.position += 1;
                    } else {
                        break;
                    }
                }
                if start == self.position {
                    bail!("line {}: expected a key name", self.line());
                }
                Ok(self.chars[start..self.position].iter().collect())
            }
        }
    }

    fn list(&mut self) -> Result<Value> {
        let start = self.line();
        self.position += 2;
        let mut items = Vec::new();
        loop {
            self.skip_space(true)?;
            match self.peek() {
                None => bail!("line {start}: a @( list is never closed with )"),
                Some(')') => {
                    self.position += 1;
                    return Ok(Value::Sequence(items));
                }
                Some(',' | ';') => {
                    self.position += 1;
                    continue;
                }
                Some(_) => {}
            }
            //Inside @( ), commas separate items rather than building a nested list.
            items.push(self.single_value()?);
        }
    }

    fn single_quoted(&mut self) -> Result<String> {
        let start = self.line();
        self.position += 1;
        let mut text = String::new();
        loop {
            match self.peek() {
                None => bail!("line {start}: a ' string is never closed"),
                Some('\'') if self.peek_at(1) == Some('\'') => {
                    text.push('\'');
                    self.position += 2;
                }
                Some('\'') => {
                    self.position += 1;
                    return Ok(text);
                }
                Some(c) => {
                    text.push(c);
                    self.position += 1;
                }
            }
        }
    }

    //Double-quoted strings: backtick escapes are honoured. $variables inside are
    //not expanded; a data file has none to expand.
    fn double_quoted(&mut self) -> Result<String> {
        let start = self.line();
        self.position += 1;
        let mut text = String::new();
        loop {
            match self.peek() {
                None => bail!("line {start}: a \" string is never closed"),
                Some('"') if self.peek_at(1) == Some('"') => {
                    text.push('"');
                    self.position += 2;
                }
                Some('"') => {
                    self.position += 1;
                    return Ok(text);
                }
                Some('`') => {
                    let escaped = self.peek_at(1).unwrap_or('`');
                    text.push(match escaped {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        '0' => '\0',
                        other => other,
                    });
                    self.position += 2;
                }
                Some(c) => {
                    text.push(c);
                    self.position += 1;
                }
            }
        }
    }

    //@' ... '@ and @" ... "@. The text starts on the line after the opener and
    //ends before the line holding the closer, which must start that line.
    fn here_string(&mut self) -> Result<String> {
        let start = self.line();
        let quote = self.peek_at(1).unwrap_or('\'');
        self.position += 2;
        while let Some(c) = self.peek() {
            self.position += 1;
            if c == '\n' {
                break;
            }
        }
        let mut lines: Vec<String> = Vec::new();
        loop {
            if self.position >= self.chars.len() {
                bail!("line {start}: a here-string is never closed with {quote}@");
            }
            let line_start = self.position;
            while let Some(c) = self.peek() {
                if c == '\n' {
                    break;
                }
                self.position += 1;
            }
            let line: String = self.chars[line_start..self.position].iter().collect();
            let line = line.strip_suffix('\r').unwrap_or(&line).to_string();
            if self.peek() == Some('\n') {
                self.position += 1;
            }
            if line.starts_with(&format!("{quote}@")) {
                //Step back to just after the closer, so trailing text is parsed.
                self.position = line_start + 2;
                return Ok(lines.join("\n"));
            }
            lines.push(line);
        }
    }

    fn variable(&mut self) -> Result<Value> {
        let start = self.position;
        self.position += 1;
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' {
                self.position += 1;
            } else {
                break;
            }
        }
        let name: String = self.chars[start..self.position].iter().collect();
        match name.to_lowercase().as_str() {
            "$true" => Ok(Value::Bool(true)),
            "$false" => Ok(Value::Bool(false)),
            "$null" => Ok(Value::Null),
            _ => bail!(
                "line {}: {name} is a variable, and a data file can only hold $true, $false and $null",
                self.line()
            ),
        }
    }

    fn number(&mut self) -> Result<Value> {
        let start = self.position;
        if self.peek() == Some('-') {
            self.position += 1;
        }
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '.' || c == '_' {
                self.position += 1;
            } else {
                break;
            }
        }
        let digits: String = self.chars[start..self.position]
            .iter()
            .filter(|c| **c != '_')
            .collect();
        let suffix_start = self.position;
        while let Some(c) = self.peek() {
            if c.is_ascii_alphabetic() {
                self.position += 1;
            } else {
                break;
            }
        }
        let suffix: String = self.chars[suffix_start..self.position]
            .iter()
            .collect::<String>()
            .to_lowercase();
        let multiplier: i64 = match suffix.as_str() {
            "" => 1,
            "kb" => 1 << 10,
            "mb" => 1 << 20,
            "gb" => 1 << 30,
            "tb" => 1 << 40,
            other => bail!("line {}: unknown number suffix '{other}'", self.line()),
        };
        if let Ok(whole) = digits.parse::<i64>() {
            return Ok(Value::Number(Number::from(whole * multiplier)));
        }
        match digits.parse::<f64>() {
            Ok(fraction) => Ok(Value::Number(Number::from(fraction * multiplier as f64))),
            Err(_) => bail!("line {}: '{digits}' is not a number", self.line()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get<'a>(map: &'a Mapping, key: &str) -> &'a Value {
        map.get(Value::String(key.to_string())).unwrap()
    }

    #[test]
    fn reads_tables_lists_and_scalars() {
        let map = parse(
            "<# header\n with lines #>\n@{\n  Name = 'it''s' # trailing\n  Count = 12; Big = 2GB\n  On = $true\n  Off = $FALSE\n  None = $null\n  List = @('a', 'b'\n    'c')\n  Bare = 'x', 'y'\n  Nested = @{ Deep = \"tab`there\" }\n}\n",
        )
        .unwrap();
        assert_eq!(get(&map, "Name"), &Value::from("it's"));
        assert_eq!(get(&map, "Count"), &Value::from(12));
        assert_eq!(get(&map, "Big"), &Value::from(2_i64 << 30));
        assert_eq!(get(&map, "On"), &Value::Bool(true));
        assert_eq!(get(&map, "Off"), &Value::Bool(false));
        assert_eq!(get(&map, "None"), &Value::Null);
        assert_eq!(get(&map, "List").as_sequence().unwrap().len(), 3);
        assert_eq!(get(&map, "Bare").as_sequence().unwrap().len(), 2);
        let nested = get(&map, "Nested").as_mapping().unwrap();
        assert_eq!(get(nested, "Deep"), &Value::from("tab\there"));
    }

    #[test]
    fn reads_here_strings() {
        let map = parse("@{\n  Text = @'\nline one\n  'quoted' line\n'@\n  After = 1\n}").unwrap();
        assert_eq!(get(&map, "Text"), &Value::from("line one\n  'quoted' line"));
        assert_eq!(get(&map, "After"), &Value::from(1));
    }

    #[test]
    fn every_embedded_blueprint_parses() {
        for entry in std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/blueprints")).unwrap()
        {
            let manifest = entry.unwrap().path().join("blueprint.psd1");
            if manifest.is_file() {
                let text = std::fs::read_to_string(&manifest).unwrap();
                let map = parse(&text).unwrap_or_else(|e| panic!("{}: {e}", manifest.display()));
                assert!(
                    map.contains_key(Value::from("Name")),
                    "{}",
                    manifest.display()
                );
            }
        }
    }

    #[test]
    fn rejects_unknown_variables() {
        assert!(parse("@{ A = $HOME }").is_err());
    }
}
