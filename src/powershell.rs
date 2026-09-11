//! String literals embedded in generated PowerShell source.

/// Match PowerShell's `CodeGeneration.EscapeSingleQuotedStringContent`:
/// ASCII apostrophes and U+2018–U+201B all delimit single-quoted strings.
/// Duplicate the original character so the literal preserves its exact value.
pub fn quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for character in value.chars() {
        quoted.push(character);
        if matches!(character, '\'' | '\u{2018}'..='\u{201b}') {
            quoted.push(character);
        }
    }
    quoted.push('\'');
    quoted
}
