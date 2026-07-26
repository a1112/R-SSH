#![allow(dead_code)]

use std::fmt;

use crate::window::NativeConfigOverrides;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SourceLocation {
    pub(crate) line: usize,
    pub(crate) column: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum StaticLuaValue {
    Nil,
    Bool(bool),
    Integer(i64),
    Number(f64),
    String(String),
    Array(Vec<StaticLuaValue>),
    Table(Vec<(StaticLuaKey, StaticLuaValue)>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StaticLuaKey {
    String(String),
    Integer(i64),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StaticNativeConfigAssignment {
    pub(crate) field_path: Vec<String>,
    pub(crate) value: StaticLuaValue,
    pub(crate) value_source: String,
    pub(crate) location: SourceLocation,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum NativeConfigLoadError {
    InvalidSyntax {
        location: SourceLocation,
        message: String,
    },
    UnsupportedDynamicLua {
        location: SourceLocation,
        message: String,
    },
    UnknownField {
        location: SourceLocation,
        field: String,
    },
    InvalidFieldValue {
        location: SourceLocation,
        field: String,
        message: String,
    },
    InternalValidation {
        location: SourceLocation,
        message: String,
    },
}

impl fmt::Display for NativeConfigLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSyntax { location, message } => {
                write!(
                    formatter,
                    "{}:{}: invalid syntax: {message}",
                    location.line, location.column
                )
            }
            Self::UnsupportedDynamicLua { location, message } => write!(
                formatter,
                "{}:{}: unsupported dynamic Lua: {message}",
                location.line, location.column
            ),
            Self::UnknownField { location, field } => write!(
                formatter,
                "{}:{}: unknown config field `{field}`",
                location.line, location.column
            ),
            Self::InvalidFieldValue {
                location,
                field,
                message,
            } => write!(
                formatter,
                "{}:{}: invalid value for `{field}`: {message}",
                location.line, location.column
            ),
            Self::InternalValidation { location, message } => write!(
                formatter,
                "{}:{}: internal validation failed: {message}",
                location.line, location.column
            ),
        }
    }
}

impl std::error::Error for NativeConfigLoadError {}

pub(crate) fn validate_cli_config_overrides(
    items: &[(String, String)],
) -> Result<Vec<StaticNativeConfigAssignment>, NativeConfigLoadError> {
    let mut assignments = Vec::with_capacity(items.len());
    for (field, source) in items {
        let mut parser = Parser::new(source);
        if parser.source.starts_with('\u{feff}') {
            parser.offset += '\u{feff}'.len_utf8();
        }
        parser.skip_trivia()?;
        let location = parser.location();
        let value = parser.parse_config_field_value(field)?;
        parser.skip_trivia()?;
        if !parser.is_eof() {
            return Err(parser.dynamic("unexpected trailing tokens after static CLI value"));
        }
        let assignment = StaticNativeConfigAssignment {
            field_path: vec![field.clone()],
            value,
            value_source: source.clone(),
            location,
        };
        validate_assignment(&assignment)?;
        assignments.push(assignment);
    }
    Ok(assignments)
}

pub(crate) fn parse_native_config_document(
    source: &str,
    cli: &[StaticNativeConfigAssignment],
) -> Result<NativeConfigOverrides, NativeConfigLoadError> {
    let mut assignments = Parser::new(source).parse_document()?;
    assignments.extend_from_slice(cli);
    if assignments.is_empty() {
        return Ok(NativeConfigOverrides::default());
    }

    for assignment in &assignments {
        validate_assignment(assignment)?;
    }
    let canonical = canonical_document(&assignments);
    crate::window::native_config_overrides_from_wezterm_lua_config(&canonical).ok_or_else(|| {
        NativeConfigLoadError::InternalValidation {
            location: SourceLocation { line: 1, column: 1 },
            message: "legacy extractor rejected strictly validated config".to_owned(),
        }
    })
}

fn validate_assignment(
    assignment: &StaticNativeConfigAssignment,
) -> Result<(), NativeConfigLoadError> {
    let field = assignment.field_path.join(".");
    let result = match field.as_str() {
        "term" | "default_cwd" | "color_scheme" => validate_non_empty_string(&assignment.value),
        "initial_cols" | "initial_rows" => {
            validate_integer_range(&assignment.value, 1, u16::MAX as u64)
        }
        "scrollback_lines" | "max_fps" => {
            validate_integer_range(&assignment.value, 0, usize::MAX as u64)
        }
        "automatically_reload_config" | "enable_tab_bar" => validate_bool(&assignment.value),
        "default_prog" | "default_gui_startup_args" => validate_string_array(&assignment.value),
        "colors" => validate_colors(&assignment.value),
        "set_environment_variables" => validate_environment(&assignment.value),
        "keys" => validate_keys(&assignment.value),
        _ => {
            return Err(NativeConfigLoadError::UnknownField {
                location: assignment.location,
                field,
            });
        }
    };
    result.map_err(|message| NativeConfigLoadError::InvalidFieldValue {
        location: assignment.location,
        field,
        message,
    })
}

fn validate_non_empty_string(value: &StaticLuaValue) -> Result<(), String> {
    match value {
        StaticLuaValue::String(value) if !value.is_empty() => Ok(()),
        StaticLuaValue::String(_) => Err("must not be empty".to_owned()),
        _ => Err("expected a string".to_owned()),
    }
}

fn validate_bool(value: &StaticLuaValue) -> Result<(), String> {
    match value {
        StaticLuaValue::Bool(_) => Ok(()),
        _ => Err("expected a boolean".to_owned()),
    }
}

fn validate_integer_range(
    value: &StaticLuaValue,
    minimum: u64,
    maximum: u64,
) -> Result<(), String> {
    let StaticLuaValue::Integer(value) = value else {
        return Err("expected an integer".to_owned());
    };
    let value = u64::try_from(*value).map_err(|_| "integer must not be negative".to_owned())?;
    if (minimum..=maximum).contains(&value) {
        Ok(())
    } else {
        Err(format!("integer must be in {minimum}..={maximum}"))
    }
}

fn validate_string_array(value: &StaticLuaValue) -> Result<(), String> {
    match value {
        StaticLuaValue::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                if !matches!(value, StaticLuaValue::String(_)) {
                    return Err(format!("array item {} must be a string", index + 1));
                }
            }
            Ok(())
        }
        StaticLuaValue::Table(entries) if entries.is_empty() => Ok(()),
        _ => Err("expected an array of strings".to_owned()),
    }
}

fn validate_colors(value: &StaticLuaValue) -> Result<(), String> {
    let entries = table_entries(value, "colors")?;
    reject_duplicate_keys(entries, "colors")?;
    for (key, value) in entries {
        let key = string_key(key, "colors")?;
        match key {
            "foreground" | "background" | "cursor_bg" | "cursor_fg" | "cursor_border"
            | "compose_cursor" | "selection_bg" => validate_color(value)?,
            "selection_fg" => match value {
                StaticLuaValue::String(value) if value.eq_ignore_ascii_case("none") => {}
                _ => validate_color(value)?,
            },
            "ansi" | "brights" => validate_color_array(value, key)?,
            "tab_bar" => validate_tab_bar(value)?,
            _ => return Err(format!("unknown colors key `{key}`")),
        }
    }
    Ok(())
}

fn validate_color(value: &StaticLuaValue) -> Result<(), String> {
    let StaticLuaValue::String(value) = value else {
        return Err("color must be a string".to_owned());
    };
    value
        .parse::<wezterm_color_types::SrgbaTuple>()
        .map(|_| ())
        .map_err(|_| format!("invalid color `{value}`"))
}

fn validate_color_array(value: &StaticLuaValue, field: &str) -> Result<(), String> {
    let StaticLuaValue::Array(values) = value else {
        return Err(format!("{field} must be an array"));
    };
    if values.len() != 8 {
        return Err(format!("{field} must contain exactly 8 colors"));
    }
    for value in values {
        validate_color(value)?;
    }
    Ok(())
}

fn validate_tab_bar(value: &StaticLuaValue) -> Result<(), String> {
    let entries = table_entries(value, "colors.tab_bar")?;
    reject_duplicate_keys(entries, "colors.tab_bar")?;
    for (key, value) in entries {
        let key = string_key(key, "colors.tab_bar")?;
        match key {
            "background" | "inactive_tab_edge" => validate_color(value)?,
            "active_tab" | "inactive_tab" | "inactive_tab_hover" | "new_tab" | "new_tab_hover" => {
                validate_tab_bar_item(value, key)?
            }
            _ => return Err(format!("unknown colors.tab_bar key `{key}`")),
        }
    }
    Ok(())
}

fn validate_tab_bar_item(value: &StaticLuaValue, item: &str) -> Result<(), String> {
    let entries = table_entries(value, item)?;
    reject_duplicate_keys(entries, item)?;
    for (key, value) in entries {
        let key = string_key(key, item)?;
        match key {
            "fg_color" | "bg_color" => validate_color(value)?,
            "intensity" => validate_enum_string(value, &["Normal", "Bold", "Half"])?,
            "underline" => validate_enum_string(
                value,
                &["None", "Single", "Double", "Curly", "Dotted", "Dashed"],
            )?,
            "italic" | "strikethrough" => validate_bool(value)?,
            _ => return Err(format!("unknown colors.tab_bar.{item} key `{key}`")),
        }
    }
    Ok(())
}

fn validate_enum_string(value: &StaticLuaValue, allowed: &[&str]) -> Result<(), String> {
    let StaticLuaValue::String(value) = value else {
        return Err("expected a string".to_owned());
    };
    if allowed.contains(&value.as_str()) {
        Ok(())
    } else {
        Err(format!("unsupported value `{value}`"))
    }
}

fn validate_environment(value: &StaticLuaValue) -> Result<(), String> {
    let entries = table_entries(value, "set_environment_variables")?;
    reject_duplicate_keys(entries, "set_environment_variables")?;
    for (key, value) in entries {
        let key = string_key(key, "set_environment_variables")?;
        if key.is_empty() {
            return Err("environment variable name must not be empty".to_owned());
        }
        if !matches!(value, StaticLuaValue::String(_)) {
            return Err(format!(
                "environment variable `{key}` value must be a string"
            ));
        }
    }
    Ok(())
}

fn validate_keys(value: &StaticLuaValue) -> Result<(), String> {
    let values = match value {
        StaticLuaValue::Array(values) => values.as_slice(),
        StaticLuaValue::Table(entries) if entries.is_empty() => &[],
        _ => return Err("keys must be an array".to_owned()),
    };
    for (index, value) in values.iter().enumerate() {
        validate_key_entry(value)
            .map_err(|message| format!("keys item {}: {message}", index + 1))?;
    }
    Ok(())
}

fn validate_key_entry(value: &StaticLuaValue) -> Result<(), String> {
    let entries = table_entries(value, "key entry")?;
    reject_duplicate_keys(entries, "key entry")?;
    let mut key_seen = false;
    let mut modifiers_seen = false;
    let mut action_seen = false;
    for (key, value) in entries {
        match string_key(key, "key entry")? {
            "key" => {
                validate_non_empty_string(value)?;
                key_seen = true;
            }
            "mods" | "mod" => {
                if modifiers_seen {
                    return Err("duplicate modifier field via `mods`/`mod` alias".to_owned());
                }
                validate_modifiers(value)?;
                modifiers_seen = true;
            }
            "action" => {
                validate_action(value)?;
                action_seen = true;
            }
            key => return Err(format!("unknown key entry field `{key}`")),
        }
    }
    if !key_seen {
        return Err("missing `key`".to_owned());
    }
    if !action_seen {
        return Err("missing `action`".to_owned());
    }
    Ok(())
}

fn validate_modifiers(value: &StaticLuaValue) -> Result<(), String> {
    let StaticLuaValue::String(value) = value else {
        return Err("mods must be a string".to_owned());
    };
    if value.eq_ignore_ascii_case("NONE") {
        return Ok(());
    }
    for modifier in value.split(['|', '+']) {
        if !matches!(
            modifier.trim().to_ascii_uppercase().as_str(),
            "CTRL" | "SHIFT" | "ALT" | "SUPER" | "LEADER" | "CMD" | "WIN" | "OPT" | "META"
        ) {
            return Err(format!("unsupported modifier `{modifier}`"));
        }
    }
    Ok(())
}

fn validate_action(value: &StaticLuaValue) -> Result<(), String> {
    let entries = table_entries(value, "action")?;
    if entries.len() != 1 {
        return Err("action must contain exactly one supported action".to_owned());
    }
    let (key, payload) = &entries[0];
    match string_key(key, "action")? {
        "SendString" if matches!(payload, StaticLuaValue::String(_)) => Ok(()),
        "SendString" => Err("SendString payload must be a string".to_owned()),
        action => Err(format!("unsupported action `{action}`")),
    }
}

fn table_entries<'a>(
    value: &'a StaticLuaValue,
    context: &str,
) -> Result<&'a [(StaticLuaKey, StaticLuaValue)], String> {
    match value {
        StaticLuaValue::Table(entries) => Ok(entries),
        _ => Err(format!("{context} must be a table")),
    }
}

fn string_key<'a>(key: &'a StaticLuaKey, context: &str) -> Result<&'a str, String> {
    match key {
        StaticLuaKey::String(key) => Ok(key),
        StaticLuaKey::Integer(key) => Err(format!("{context} does not support integer key {key}")),
    }
}

fn reject_duplicate_keys(
    entries: &[(StaticLuaKey, StaticLuaValue)],
    context: &str,
) -> Result<(), String> {
    for (index, (key, _)) in entries.iter().enumerate() {
        if entries[..index].iter().any(|(previous, _)| previous == key) {
            return Err(format!("{context} contains duplicate key {key:?}"));
        }
    }
    Ok(())
}

fn canonical_document(assignments: &[StaticNativeConfigAssignment]) -> String {
    let mut output = String::from("return {\n");
    for (index, assignment) in assignments.iter().enumerate() {
        if assignments[index + 1..]
            .iter()
            .any(|later| later.field_path == assignment.field_path)
        {
            continue;
        }
        output.push_str(&assignment.field_path[0]);
        output.push('=');
        let context = if assignment.field_path[0] == "keys" {
            StaticValueContext::KeyBindings
        } else {
            StaticValueContext::General
        };
        write_canonical_value_with_context(&assignment.value, context, &mut output);
        output.push_str(",\n");
    }
    output.push('}');
    output
}

fn write_canonical_value(value: &StaticLuaValue, output: &mut String) {
    write_canonical_value_with_context(value, StaticValueContext::General, output);
}

fn write_canonical_value_with_context(
    value: &StaticLuaValue,
    context: StaticValueContext,
    output: &mut String,
) {
    match value {
        StaticLuaValue::Nil => output.push_str("nil"),
        StaticLuaValue::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        StaticLuaValue::Integer(value) => output.push_str(&value.to_string()),
        StaticLuaValue::Number(value) => output.push_str(&value.to_string()),
        StaticLuaValue::String(value) => {
            output.push('"');
            for character in value.chars() {
                match character {
                    '\\' => output.push_str("\\\\"),
                    '"' => output.push_str("\\\""),
                    '\n' => output.push_str("\\n"),
                    '\r' => output.push_str("\\r"),
                    '\t' => output.push_str("\\t"),
                    character if character.is_control() => {
                        output.push_str(&format!("\\u{{{:x}}}", character as u32));
                    }
                    character => output.push(character),
                }
            }
            output.push('"');
        }
        StaticLuaValue::Array(values) => {
            output.push('{');
            for value in values {
                write_canonical_value_with_context(value, context, output);
                output.push(',');
            }
            output.push('}');
        }
        StaticLuaValue::Table(entries) => {
            output.push('{');
            for (key, value) in entries {
                match key {
                    StaticLuaKey::String(key)
                        if is_identifier_start(key.chars().next().unwrap_or('_'))
                            && key.chars().all(is_identifier_continue) =>
                    {
                        if is_lua_reserved_keyword(key) {
                            output.push('[');
                            write_canonical_value(&StaticLuaValue::String(key.clone()), output);
                            output.push(']');
                        } else {
                            output.push_str(key);
                        }
                    }
                    StaticLuaKey::String(key) => {
                        output.push('[');
                        write_canonical_value(&StaticLuaValue::String(key.clone()), output);
                        output.push(']');
                    }
                    StaticLuaKey::Integer(key) => {
                        output.push('[');
                        output.push_str(&key.to_string());
                        output.push(']');
                    }
                }
                output.push('=');
                if let Some(payload) = (context == StaticValueContext::KeyBindings
                    && matches!(key, StaticLuaKey::String(key) if key == "action"))
                .then(|| static_send_string_payload(value))
                .flatten()
                {
                    write_canonical_action(payload, output);
                } else {
                    write_canonical_value_with_context(value, context, output);
                }
                output.push(',');
            }
            output.push('}');
        }
    }
}

fn static_send_string_payload(value: &StaticLuaValue) -> Option<&str> {
    let StaticLuaValue::Table(entries) = value else {
        return None;
    };
    match entries.as_slice() {
        [(StaticLuaKey::String(action), StaticLuaValue::String(payload))]
            if action == "SendString" =>
        {
            Some(payload)
        }
        _ => None,
    }
}

fn write_canonical_action(payload: &str, output: &mut String) {
    output.push_str("wezterm.action.SendString(");
    write_canonical_value(&StaticLuaValue::String(payload.to_owned()), output);
    output.push(')');
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StaticValueContext {
    General,
    KeyBindings,
}

struct Parser<'a> {
    source: &'a str,
    offset: usize,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str) -> Self {
        Self { source, offset: 0 }
    }

    fn parse_document(
        mut self,
    ) -> Result<Vec<StaticNativeConfigAssignment>, NativeConfigLoadError> {
        if self.source.starts_with('\u{feff}') {
            self.offset += '\u{feff}'.len_utf8();
        }
        self.skip_trivia()?;
        if self.consume_keyword("return") {
            self.skip_trivia()?;
            if self.peek() != Some('{') {
                return Err(self.dynamic("dynamic return root; expected a static table"));
            }
            let assignments = self.parse_root_config_table()?;
            self.skip_trivia()?;
            if !self.is_eof() {
                return Err(self.dynamic("unexpected trailing top-level statement"));
            }
            return Ok(assignments);
        }

        self.expect_keyword("local")?;
        self.skip_trivia()?;
        if self.parse_identifier()? != "config" {
            return Err(self.dynamic(
                "only the static config builder declaration is allowed before top-level statements",
            ));
        }
        self.skip_trivia()?;
        self.expect_char('=')?;
        self.skip_trivia()?;
        self.parse_builder_expression()?;
        self.skip_trivia()?;
        self.consume_char(';');

        let mut assignments = Vec::new();
        loop {
            self.skip_trivia()?;
            if self.consume_keyword("return") {
                self.skip_trivia()?;
                self.expect_identifier("config")?;
                self.skip_trivia()?;
                if !self.is_eof() {
                    return Err(self.syntax("unexpected trailing top-level statement"));
                }
                return Ok(assignments);
            }

            let location = self.location();
            if self.parse_identifier()? != "config" {
                return Err(self.dynamic(
                    "only direct `config.FIELD = STATIC_VALUE` top-level statements are allowed",
                ));
            }
            self.expect_char('.')?;
            let field = self.parse_identifier()?;
            self.skip_trivia()?;
            self.expect_char('=')?;
            self.skip_trivia()?;
            let value_start = self.offset;
            let value = self.parse_config_field_value(&field)?;
            let value_source = self.source[value_start..self.offset].to_owned();
            assignments.push(StaticNativeConfigAssignment {
                field_path: vec![field],
                value,
                value_source,
                location,
            });
            self.skip_trivia()?;
            self.consume_char(';');
        }
    }

    fn parse_builder_expression(&mut self) -> Result<(), NativeConfigLoadError> {
        self.expect_identifier("require")?;
        self.skip_trivia()?;
        let module = if self.consume_char('(') {
            self.skip_trivia()?;
            let module = self.parse_string()?;
            self.skip_trivia()?;
            self.expect_char(')')?;
            module
        } else {
            self.parse_string()?
        };
        if module != "wezterm" {
            return Err(self.dynamic("config builder must require `wezterm`"));
        }
        self.skip_trivia()?;
        self.expect_char('.')?;
        self.expect_identifier("config_builder")?;
        self.skip_trivia()?;
        self.expect_char('(')?;
        self.skip_trivia()?;
        self.expect_char(')')
    }

    fn parse_root_config_table(
        &mut self,
    ) -> Result<Vec<StaticNativeConfigAssignment>, NativeConfigLoadError> {
        self.expect_char('{')?;
        self.skip_trivia()?;
        let mut assignments = Vec::new();
        while !self.consume_char('}') {
            if self.is_eof() {
                return Err(self.syntax("unterminated config table"));
            }
            let location = self.location();
            let field = if self.peek() == Some('[') && self.long_bracket_level().is_none() {
                self.bump();
                self.skip_trivia()?;
                let field = match self.parse_value()? {
                    StaticLuaValue::String(field) => field,
                    _ => return Err(self.syntax("config table bracket key must be a string")),
                };
                self.skip_trivia()?;
                self.expect_char(']')?;
                field
            } else {
                self.parse_identifier()?
            };
            self.skip_trivia()?;
            self.expect_char('=')?;
            self.skip_trivia()?;
            let value_start = self.offset;
            let value = self.parse_config_field_value(&field)?;
            assignments.push(StaticNativeConfigAssignment {
                field_path: vec![field],
                value,
                value_source: self.source[value_start..self.offset].to_owned(),
                location,
            });
            self.skip_trivia()?;
            if self.is_eof() {
                return Err(self.syntax("unterminated config table"));
            }
            if self.consume_char(',') || self.consume_char(';') {
                self.skip_trivia()?;
            } else if self.peek() != Some('}') {
                return Err(self.syntax("expected `,`, `;`, or `}` in config table"));
            }
        }
        Ok(assignments)
    }

    fn parse_config_field_value(
        &mut self,
        field: &str,
    ) -> Result<StaticLuaValue, NativeConfigLoadError> {
        let context = if field == "keys" {
            StaticValueContext::KeyBindings
        } else {
            StaticValueContext::General
        };
        self.parse_value_with_context(context)
    }

    fn parse_value(&mut self) -> Result<StaticLuaValue, NativeConfigLoadError> {
        self.parse_value_with_context(StaticValueContext::General)
    }

    fn parse_value_with_context(
        &mut self,
        context: StaticValueContext,
    ) -> Result<StaticLuaValue, NativeConfigLoadError> {
        self.skip_trivia()?;
        match self.peek() {
            Some('\'') | Some('"') => self.parse_string().map(StaticLuaValue::String),
            Some('[') if self.long_bracket_level().is_some() => {
                self.parse_long_string().map(StaticLuaValue::String)
            }
            Some('{') => self.parse_table(context),
            Some('-' | '+' | '.' | '0'..='9') => self.parse_number(),
            Some(_) if self.consume_keyword("true") => Ok(StaticLuaValue::Bool(true)),
            Some(_) if self.consume_keyword("false") => Ok(StaticLuaValue::Bool(false)),
            Some(_) if self.consume_keyword("nil") => Ok(StaticLuaValue::Nil),
            Some(character) if is_identifier_start(character) => {
                Err(self.dynamic("variable-derived values are unsupported"))
            }
            Some(_) => Err(self.dynamic("value must be a static literal")),
            None => Err(self.syntax("expected static value")),
        }
    }

    fn parse_table(
        &mut self,
        context: StaticValueContext,
    ) -> Result<StaticLuaValue, NativeConfigLoadError> {
        self.expect_char('{')?;
        self.skip_trivia()?;
        if self.consume_char('}') {
            return Ok(StaticLuaValue::Table(Vec::new()));
        }
        let mut keyed_entries = Vec::new();
        let mut array_entries = Vec::new();
        while !self.consume_char('}') {
            if self.is_eof() {
                return Err(self.syntax("unterminated table literal"));
            }
            let item_start = self.offset;
            let named_key = if self.peek().is_some_and(is_identifier_start) {
                let key = self.parse_identifier()?;
                self.skip_trivia()?;
                if self.consume_char('=') {
                    Some(StaticLuaKey::String(key))
                } else {
                    self.offset = item_start;
                    None
                }
            } else {
                None
            };
            let bracket_key = if named_key.is_none()
                && self.peek() == Some('[')
                && self.long_bracket_level().is_none()
            {
                self.bump();
                let key = match self.parse_value()? {
                    StaticLuaValue::String(key) => StaticLuaKey::String(key),
                    StaticLuaValue::Integer(key) => StaticLuaKey::Integer(key),
                    _ => {
                        return Err(self.syntax("table bracket key must be a string or integer"));
                    }
                };
                self.skip_trivia()?;
                self.expect_char(']')?;
                self.skip_trivia()?;
                self.expect_char('=')?;
                Some(key)
            } else {
                None
            };
            if let Some(key) = named_key.or(bracket_key) {
                if !array_entries.is_empty() {
                    return Err(self.syntax("mixed keyed and positional tables are unsupported"));
                }
                let is_action = matches!(&key, StaticLuaKey::String(key) if key == "action");
                let value = if context == StaticValueContext::KeyBindings && is_action {
                    match self.parse_static_action()? {
                        Some(action) => action,
                        None => {
                            return Err(self.dynamic(
                                "action must be exactly `wezterm.action.SendString(STRING)`",
                            ));
                        }
                    }
                } else {
                    self.parse_value_with_context(context)?
                };
                keyed_entries.push((key, value));
            } else {
                if !keyed_entries.is_empty() {
                    return Err(self.syntax("mixed keyed and positional tables are unsupported"));
                }
                array_entries.push(self.parse_value_with_context(context)?);
            }
            self.skip_trivia()?;
            if self.is_eof() {
                return Err(self.syntax("unterminated table literal"));
            }
            if self.consume_char(',') || self.consume_char(';') {
                self.skip_trivia()?;
            } else if self.peek() != Some('}') {
                return Err(self.syntax("expected `,`, `;`, or `}`"));
            }
        }
        if keyed_entries.is_empty() && !array_entries.is_empty() {
            Ok(StaticLuaValue::Array(array_entries))
        } else {
            Ok(StaticLuaValue::Table(keyed_entries))
        }
    }

    fn parse_static_action(&mut self) -> Result<Option<StaticLuaValue>, NativeConfigLoadError> {
        let start = self.offset;
        self.skip_trivia()?;
        if !self.consume_keyword("wezterm") {
            self.offset = start;
            return Ok(None);
        }
        if self.expect_char('.').is_err()
            || self.expect_identifier("action").is_err()
            || self.expect_char('.').is_err()
            || self.expect_identifier("SendString").is_err()
        {
            return Err(self.dynamic("action must be exactly `wezterm.action.SendString(STRING)`"));
        }
        self.skip_trivia()?;
        if !self.consume_char('(') {
            return Err(self.dynamic("action must be exactly `wezterm.action.SendString(STRING)`"));
        }
        let payload = self.parse_value()?;
        if !matches!(payload, StaticLuaValue::String(_)) {
            return Err(self.dynamic("SendString payload must be one static string"));
        }
        self.skip_trivia()?;
        if !self.consume_char(')') {
            return Err(self.dynamic("SendString requires exactly one parenthesized string"));
        }
        Ok(Some(StaticLuaValue::Table(vec![(
            StaticLuaKey::String("SendString".to_owned()),
            payload,
        )])))
    }

    fn parse_string(&mut self) -> Result<String, NativeConfigLoadError> {
        let quote = self
            .bump()
            .filter(|character| matches!(character, '\'' | '"'))
            .ok_or_else(|| self.syntax("expected string literal"))?;
        let mut output = String::new();
        while let Some(character) = self.bump() {
            if character == quote {
                return Ok(output);
            }
            if character != '\\' {
                if matches!(character, '\n' | '\r') {
                    return Err(self.syntax("short string literals cannot contain raw newlines"));
                }
                output.push(character);
                continue;
            }
            let escaped = self
                .bump()
                .ok_or_else(|| self.syntax("unterminated string escape"))?;
            match escaped {
                'a' => output.push('\x07'),
                'b' => output.push('\x08'),
                'f' => output.push('\x0c'),
                'n' => output.push('\n'),
                'r' => output.push('\r'),
                't' => output.push('\t'),
                'v' => output.push('\x0b'),
                '\\' => output.push('\\'),
                '\'' => output.push('\''),
                '"' => output.push('"'),
                '\n' => output.push('\n'),
                '\r' => {
                    self.consume_char('\n');
                    output.push('\n');
                }
                'z' => {
                    while self.peek().is_some_and(char::is_whitespace) {
                        self.bump();
                    }
                }
                'x' => {
                    let value = self.parse_fixed_radix_digits(2, 16)?;
                    output.push(
                        char::from_u32(value)
                            .ok_or_else(|| self.syntax("invalid hexadecimal string escape"))?,
                    );
                }
                'u' => {
                    self.expect_char('{')?;
                    let start = self.offset;
                    while self
                        .peek()
                        .is_some_and(|character| character.is_ascii_hexdigit())
                    {
                        self.bump();
                    }
                    if start == self.offset {
                        return Err(self.syntax("empty unicode string escape"));
                    }
                    let value = u32::from_str_radix(&self.source[start..self.offset], 16)
                        .map_err(|_| self.syntax("invalid unicode string escape"))?;
                    self.expect_char('}')?;
                    output.push(
                        char::from_u32(value)
                            .ok_or_else(|| self.syntax("invalid unicode scalar value"))?,
                    );
                }
                digit if digit.is_ascii_digit() => {
                    let mut digits = String::from(digit);
                    for _ in 0..2 {
                        if self
                            .peek()
                            .is_some_and(|character| character.is_ascii_digit())
                        {
                            digits.push(self.bump().unwrap());
                        }
                    }
                    let value = digits
                        .parse::<u32>()
                        .map_err(|_| self.syntax("invalid decimal string escape"))?;
                    output.push(
                        char::from_u32(value)
                            .filter(|_| value <= 255)
                            .ok_or_else(|| self.syntax("decimal string escape exceeds 255"))?,
                    );
                }
                other => {
                    return Err(self.syntax(&format!("unknown short string escape `\\{other}`")));
                }
            }
        }
        Err(self.syntax("unterminated string literal"))
    }

    fn parse_long_string(&mut self) -> Result<String, NativeConfigLoadError> {
        let (level, opener_len) = self
            .long_bracket_level()
            .ok_or_else(|| self.syntax("expected long string"))?;
        self.offset += opener_len;
        let content_start = self.offset;
        let closer = format!("]{}]", "=".repeat(level));
        let Some(relative_end) = self.remaining().find(&closer) else {
            return Err(self.syntax("unterminated long string"));
        };
        let content_end = self.offset + relative_end;
        let mut content = &self.source[content_start..content_end];
        if let Some(after_newline) = content.strip_prefix("\r\n") {
            content = after_newline;
        } else if let Some(after_newline) = content.strip_prefix('\n') {
            content = after_newline;
        }
        self.offset = content_end + closer.len();
        Ok(content.to_owned())
    }

    fn parse_number(&mut self) -> Result<StaticLuaValue, NativeConfigLoadError> {
        let start = self.offset;
        self.consume_char('+');
        self.consume_char('-');
        let mut has_digits = false;
        while self
            .peek()
            .is_some_and(|character| character.is_ascii_digit())
        {
            has_digits = true;
            self.bump();
        }
        let mut is_float = false;
        if self.consume_char('.') {
            is_float = true;
            while self
                .peek()
                .is_some_and(|character| character.is_ascii_digit())
            {
                has_digits = true;
                self.bump();
            }
        }
        if !has_digits {
            return Err(self.syntax("invalid number literal"));
        }
        if self
            .peek()
            .is_some_and(|character| matches!(character, 'e' | 'E'))
        {
            is_float = true;
            self.bump();
            if self
                .peek()
                .is_some_and(|character| matches!(character, '+' | '-'))
            {
                self.bump();
            }
            let exponent_start = self.offset;
            while self
                .peek()
                .is_some_and(|character| character.is_ascii_digit())
            {
                self.bump();
            }
            if exponent_start == self.offset {
                return Err(self.syntax("number exponent requires digits"));
            }
        }
        if self.peek().is_some_and(is_identifier_continue) {
            return Err(self.syntax("invalid number suffix"));
        }
        let text = &self.source[start..self.offset];
        if !is_float {
            return text
                .parse::<i64>()
                .map(StaticLuaValue::Integer)
                .map_err(|_| self.syntax("integer is outside the supported i64 range"));
        }
        let number = text
            .parse::<f64>()
            .map_err(|_| self.syntax("invalid floating-point number"))?;
        if number.is_finite() {
            Ok(StaticLuaValue::Number(number))
        } else {
            Err(self.syntax("non-finite numbers are unsupported"))
        }
    }

    fn parse_fixed_radix_digits(
        &mut self,
        count: usize,
        radix: u32,
    ) -> Result<u32, NativeConfigLoadError> {
        let start = self.offset;
        for _ in 0..count {
            if !self
                .peek()
                .is_some_and(|character| character.is_digit(radix))
            {
                return Err(self.syntax("incomplete string escape"));
            }
            self.bump();
        }
        u32::from_str_radix(&self.source[start..self.offset], radix)
            .map_err(|_| self.syntax("invalid string escape"))
    }

    fn skip_trivia(&mut self) -> Result<(), NativeConfigLoadError> {
        loop {
            while self.peek().is_some_and(char::is_whitespace) {
                self.bump();
            }
            if !self.remaining().starts_with("--") {
                return Ok(());
            }
            self.offset += 2;
            if self.long_bracket_level().is_some() {
                self.parse_long_string()?;
                continue;
            }
            while let Some(character) = self.bump() {
                if character == '\n' {
                    break;
                }
            }
        }
    }

    fn long_bracket_level(&self) -> Option<(usize, usize)> {
        let bytes = self.remaining().as_bytes();
        if bytes.first() != Some(&b'[') {
            return None;
        }
        let mut index = 1;
        while bytes.get(index) == Some(&b'=') {
            index += 1;
        }
        (bytes.get(index) == Some(&b'[')).then_some((index - 1, index + 1))
    }

    fn parse_identifier(&mut self) -> Result<String, NativeConfigLoadError> {
        self.skip_trivia()?;
        let Some(first) = self.peek() else {
            return Err(self.syntax("expected identifier"));
        };
        if !is_identifier_start(first) {
            return Err(self.syntax("expected identifier"));
        }
        let start = self.offset;
        self.bump();
        while self.peek().is_some_and(is_identifier_continue) {
            self.bump();
        }
        Ok(self.source[start..self.offset].to_owned())
    }

    fn expect_identifier(&mut self, expected: &str) -> Result<(), NativeConfigLoadError> {
        let actual = self.parse_identifier()?;
        if actual == expected {
            Ok(())
        } else {
            Err(self.syntax(&format!("expected `{expected}`")))
        }
    }

    fn consume_keyword(&mut self, keyword: &str) -> bool {
        let remaining = self.remaining();
        if !remaining.starts_with(keyword) {
            return false;
        }
        let end = keyword.len();
        if remaining[end..]
            .chars()
            .next()
            .is_some_and(is_identifier_continue)
        {
            return false;
        }
        self.offset += end;
        true
    }

    fn expect_keyword(&mut self, keyword: &str) -> Result<(), NativeConfigLoadError> {
        if self.consume_keyword(keyword) {
            Ok(())
        } else {
            Err(self.syntax(&format!("expected `{keyword}`")))
        }
    }

    fn consume_char(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect_char(&mut self, expected: char) -> Result<(), NativeConfigLoadError> {
        self.skip_trivia()?;
        if self.consume_char(expected) {
            Ok(())
        } else {
            Err(self.syntax(&format!("expected `{expected}`")))
        }
    }

    fn remaining(&self) -> &'a str {
        &self.source[self.offset..]
    }

    fn peek(&self) -> Option<char> {
        self.remaining().chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let character = self.peek()?;
        self.offset += character.len_utf8();
        Some(character)
    }

    fn is_eof(&self) -> bool {
        self.offset == self.source.len()
    }

    fn location(&self) -> SourceLocation {
        let prefix = &self.source[..self.offset];
        let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
        let column = prefix
            .rsplit_once('\n')
            .map_or(prefix, |(_, tail)| tail)
            .chars()
            .count()
            + 1;
        SourceLocation { line, column }
    }

    fn syntax(&self, message: &str) -> NativeConfigLoadError {
        NativeConfigLoadError::InvalidSyntax {
            location: self.location(),
            message: message.to_owned(),
        }
    }

    fn dynamic(&self, message: &str) -> NativeConfigLoadError {
        NativeConfigLoadError::UnsupportedDynamicLua {
            location: self.location(),
            message: message.to_owned(),
        }
    }
}

fn is_identifier_start(character: char) -> bool {
    character == '_' || character.is_ascii_alphabetic()
}

fn is_identifier_continue(character: char) -> bool {
    character == '_' || character.is_ascii_alphanumeric()
}

fn is_lua_reserved_keyword(value: &str) -> bool {
    matches!(
        value,
        "and"
            | "break"
            | "do"
            | "else"
            | "elseif"
            | "end"
            | "false"
            | "for"
            | "function"
            | "goto"
            | "if"
            | "in"
            | "local"
            | "nil"
            | "not"
            | "or"
            | "repeat"
            | "return"
            | "then"
            | "true"
            | "until"
            | "while"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_parser_accepts_empty_direct_table() {
        let overrides = parse_native_config_document("return {}", &[]).unwrap();

        assert_eq!(overrides, crate::window::NativeConfigOverrides::default());
    }

    #[test]
    fn strict_parser_accepts_config_builder_direct_assignments() {
        let source = r#"
            local config = require 'wezterm'.config_builder()
            config.term = 'xterm-256color'
            config.enable_tab_bar = false
            return config
        "#;

        assert!(parse_native_config_document(source, &[]).is_ok());
    }

    #[test]
    fn strict_parser_accepts_builder_and_assignment_semicolons_with_crlf_comments() {
        let source = "\u{feff}-- header\r\n\
            local config = require -- module\r\n\
            ('wezterm').config_builder() ; -- builder\r\n\
            config.term = 'xterm-256color'; -- assignment\r\n\
            config.enable_tab_bar = false ;\r\n\
            return config -- eof";

        let overrides = parse_native_config_document(source, &[]).unwrap();

        assert_eq!(overrides.term.as_deref(), Some("xterm-256color"));
        assert_eq!(overrides.enable_tab_bar, Some(false));
    }

    #[test]
    fn strict_parser_consumes_nested_tables_arrays_strings_and_comments() {
        let source = "\u{feff}--[=[ header\r\ncomment ]=]\r\nreturn {\r\n\
            term = [=[xterm-256color]=], -- trailing\r\n\
            colors = { ansi = { 'a\\n', \"b\\x21\", 3, -4.5e1, true, nil, {}, }, },\r\n\
        } -- eof";

        let assignments = Parser::new(source).parse_document().unwrap();

        assert_eq!(assignments.len(), 2);
        assert_eq!(
            assignments[0].value,
            StaticLuaValue::String("xterm-256color".to_owned())
        );
        let StaticLuaValue::Table(colors) = &assignments[1].value else {
            panic!("expected nested colors table");
        };
        assert!(matches!(colors[0].1, StaticLuaValue::Array(_)));
    }

    #[test]
    fn strict_parser_rejects_trailing_top_level_statement() {
        let error = Parser::new("return {}\nconfig.term = 'late'")
            .parse_document()
            .unwrap_err();

        assert!(matches!(
            error,
            NativeConfigLoadError::UnsupportedDynamicLua {
                location: SourceLocation { line: 2, column: 1 },
                ..
            }
        ));
    }

    #[test]
    fn strict_parser_rejects_dynamic_return_root() {
        let error = Parser::new("return config").parse_document().unwrap_err();

        assert!(matches!(
            error,
            NativeConfigLoadError::UnsupportedDynamicLua {
                location: SourceLocation { line: 1, column: 8 },
                ref message,
            } if message.contains("dynamic return root")
        ));
    }

    #[test]
    fn strict_parser_rejects_variable_derived_value() {
        let source = "local config = require('wezterm').config_builder()\n\
                      config.term = dynamic_term\n\
                      return config";
        let error = Parser::new(source).parse_document().unwrap_err();

        assert!(matches!(
            error,
            NativeConfigLoadError::UnsupportedDynamicLua { ref message, .. }
                if message.contains("variable-derived")
        ));
    }

    #[test]
    fn strict_parser_rejects_event_callback_and_table_insert() {
        let event = "local wezterm = require 'wezterm'\n\
                     wezterm.on('update-right-status', function() end)\n\
                     return {}";
        let inserted = "local config = require 'wezterm'.config_builder()\n\
                        config.keys = {}\n\
                        table.insert(config.keys, { key = 'x' })\n\
                        return config";

        for source in [event, inserted] {
            let error = Parser::new(source).parse_document().unwrap_err();
            assert!(matches!(
                error,
                NativeConfigLoadError::UnsupportedDynamicLua { ref message, .. }
                    if message.contains("top-level statements")
            ));
        }
    }

    #[test]
    fn strict_parser_rejects_malformed_balanced_value() {
        for source in [
            "return { colors = { ansi = { 'a' } }",
            "return { term = 'unterminated }",
            "return { term = [=[unterminated }",
            "--[=[ unterminated comment\nreturn {}",
        ] {
            let error = Parser::new(source).parse_document().unwrap_err();
            assert!(matches!(
                error,
                NativeConfigLoadError::InvalidSyntax { ref message, .. }
                    if message.contains("unterminated")
            ));
        }
    }

    #[test]
    fn strict_short_strings_preserve_known_escapes_and_reject_invalid_forms() {
        let assignments =
            Parser::new(r#"return { term = "\a\b\f\n\r\t\v\\\"\'\x41\065\u{42}\z   C" }"#)
                .parse_document()
                .unwrap();
        assert_eq!(
            assignments[0].value,
            StaticLuaValue::String("\x07\x08\x0c\n\r\t\x0b\\\"'AABC".to_owned())
        );

        for source in [
            r#"return { term = "bad\q" }"#,
            "return { term = \"bare\nnewline\" }",
            "return { term = \"bare\rcarriage\" }",
        ] {
            assert!(matches!(
                parse_native_config_document(source, &[]),
                Err(NativeConfigLoadError::InvalidSyntax { .. })
            ));
        }
    }

    #[test]
    fn strict_registry_accepts_lifecycle_consumer_fields() {
        let source = r##"
            return {
                term = "xterm-256color",
                default_cwd = "C:\\work\"quoted",
                initial_cols = 132,
                initial_rows = 43,
                automatically_reload_config = true,
                scrollback_lines = 9001,
                max_fps = 120,
                enable_tab_bar = false,
                color_scheme = "Builtin Solarized Dark",
                default_prog = { "pwsh", "-NoLogo" },
                default_gui_startup_args = { "ssh", "host" },
                colors = {
                    foreground = "#c0c0c0",
                    background = "#101010",
                    cursor_bg = "#ffffff",
                    cursor_fg = "#000000",
                    cursor_border = "#eeeeee",
                    compose_cursor = "#123456",
                    selection_bg = "#303030",
                    selection_fg = "none",
                    ansi = {
                        "#000000", "#800000", "#008000", "#808000",
                        "#000080", "#800080", "#008080", "#c0c0c0",
                    },
                    brights = {
                        "#808080", "#ff0000", "#00ff00", "#ffff00",
                        "#0000ff", "#ff00ff", "#00ffff", "#ffffff",
                    },
                    tab_bar = {
                        background = "#111111",
                        inactive_tab_edge = "#222222",
                        active_tab = {
                            fg_color = "#ffffff",
                            bg_color = "#333333",
                            intensity = "Bold",
                            underline = "Single",
                            italic = true,
                            strikethrough = false,
                        },
                        inactive_tab = { fg_color = "#aaaaaa", bg_color = "#222222" },
                        inactive_tab_hover = { fg_color = "#bbbbbb", bg_color = "#333333" },
                        new_tab = { fg_color = "#cccccc", bg_color = "#444444" },
                        new_tab_hover = { fg_color = "#dddddd", bg_color = "#555555" },
                    },
                },
                set_environment_variables = {
                    FOO = "bar",
                    ["WITH-DASH"] = "value",
                },
                keys = {
                    {
                        key = "x",
                        mods = "CTRL|SHIFT",
                        action = wezterm.action.SendString("safe\"\\\nvalue"),
                    },
                },
            }
        "##;

        let overrides = parse_native_config_document(source, &[]).unwrap();

        assert_eq!(overrides.term.as_deref(), Some("xterm-256color"));
        assert_eq!(overrides.default_cwd.as_deref(), Some("C:\\work\"quoted"));
        assert_eq!(overrides.initial_cols, Some(132));
        assert_eq!(overrides.initial_rows, Some(43));
        assert_eq!(overrides.automatically_reload_config, Some(true));
        assert_eq!(overrides.scrollback_lines, Some(9001));
        assert_eq!(overrides.max_fps, Some(120));
        assert_eq!(overrides.enable_tab_bar, Some(false));
        assert_eq!(
            overrides.color_scheme.as_deref(),
            Some("Builtin Solarized Dark")
        );
        assert_eq!(
            overrides.default_prog.as_deref(),
            Some(["pwsh".to_owned(), "-NoLogo".to_owned()].as_slice())
        );
        assert_eq!(
            overrides.default_gui_startup_args.as_deref(),
            Some(["ssh".to_owned(), "host".to_owned()].as_slice())
        );
        let colors = overrides.colors.as_ref().unwrap();
        assert_eq!(
            colors.foreground,
            Some(rssh_terminal::Color::Rgb(0xc0, 0xc0, 0xc0))
        );
        assert_eq!(
            colors.background,
            Some(rssh_terminal::Color::Rgb(0x10, 0x10, 0x10))
        );
        assert_eq!(
            colors.cursor_bg,
            Some(rssh_terminal::Color::Rgb(0xff, 0xff, 0xff))
        );
        assert_eq!(
            colors.cursor_fg,
            Some(rssh_terminal::Color::Rgb(0x00, 0x00, 0x00))
        );
        assert_eq!(
            colors.cursor_border,
            Some(rssh_terminal::Color::Rgb(0xee, 0xee, 0xee))
        );
        assert_eq!(
            colors.compose_cursor,
            Some(rssh_terminal::Color::Rgb(0x12, 0x34, 0x56))
        );
        assert_eq!(colors.selection_fg, Some(None));
        assert_eq!(
            colors.selection_bg,
            Some(rssh_terminal::Color::Rgb(0x30, 0x30, 0x30))
        );
        assert_eq!(
            colors.ansi,
            Some([
                rssh_terminal::Color::Rgb(0x00, 0x00, 0x00),
                rssh_terminal::Color::Rgb(0x80, 0x00, 0x00),
                rssh_terminal::Color::Rgb(0x00, 0x80, 0x00),
                rssh_terminal::Color::Rgb(0x80, 0x80, 0x00),
                rssh_terminal::Color::Rgb(0x00, 0x00, 0x80),
                rssh_terminal::Color::Rgb(0x80, 0x00, 0x80),
                rssh_terminal::Color::Rgb(0x00, 0x80, 0x80),
                rssh_terminal::Color::Rgb(0xc0, 0xc0, 0xc0),
            ])
        );
        assert_eq!(
            colors.brights,
            Some([
                rssh_terminal::Color::Rgb(0x80, 0x80, 0x80),
                rssh_terminal::Color::Rgb(0xff, 0x00, 0x00),
                rssh_terminal::Color::Rgb(0x00, 0xff, 0x00),
                rssh_terminal::Color::Rgb(0xff, 0xff, 0x00),
                rssh_terminal::Color::Rgb(0x00, 0x00, 0xff),
                rssh_terminal::Color::Rgb(0xff, 0x00, 0xff),
                rssh_terminal::Color::Rgb(0x00, 0xff, 0xff),
                rssh_terminal::Color::Rgb(0xff, 0xff, 0xff),
            ])
        );
        assert_eq!(
            colors.tab_bar_background,
            Some(rssh_terminal::Color::Rgb(0x11, 0x11, 0x11))
        );
        assert_eq!(
            colors.tab_bar_inactive_tab_edge,
            Some(rssh_terminal::Color::Rgb(0x22, 0x22, 0x22))
        );
        assert_eq!(
            colors.tab_bar_active_tab.test_projection(),
            (
                Some(rssh_terminal::Color::Rgb(0xff, 0xff, 0xff)),
                Some(rssh_terminal::Color::Rgb(0x33, 0x33, 0x33)),
                Some("Bold"),
                Some("Single"),
                Some(true),
                Some(false),
            )
        );
        assert_eq!(
            colors.tab_bar_inactive_tab.test_projection(),
            (
                Some(rssh_terminal::Color::Rgb(0xaa, 0xaa, 0xaa)),
                Some(rssh_terminal::Color::Rgb(0x22, 0x22, 0x22)),
                None,
                None,
                None,
                None,
            )
        );
        assert_eq!(
            colors.tab_bar_inactive_tab_hover.test_projection(),
            (
                Some(rssh_terminal::Color::Rgb(0xbb, 0xbb, 0xbb)),
                Some(rssh_terminal::Color::Rgb(0x33, 0x33, 0x33)),
                None,
                None,
                None,
                None,
            )
        );
        assert_eq!(
            colors.tab_bar_new_tab.test_projection(),
            (
                Some(rssh_terminal::Color::Rgb(0xcc, 0xcc, 0xcc)),
                Some(rssh_terminal::Color::Rgb(0x44, 0x44, 0x44)),
                None,
                None,
                None,
                None,
            )
        );
        assert_eq!(
            colors.tab_bar_new_tab_hover.test_projection(),
            (
                Some(rssh_terminal::Color::Rgb(0xdd, 0xdd, 0xdd)),
                Some(rssh_terminal::Color::Rgb(0x55, 0x55, 0x55)),
                None,
                None,
                None,
                None,
            )
        );
        assert_eq!(
            overrides
                .set_environment_variables
                .as_ref()
                .and_then(|environment| environment.get("WITH-DASH"))
                .map(String::as_str),
            Some("value")
        );
        assert_eq!(
            overrides
                .key_assignments
                .as_ref()
                .map(|assignments| assignments.len()),
            Some(1)
        );
        assert_eq!(
            overrides.key_assignments.as_ref().unwrap()[0].test_projection(),
            ("CTRL|SHIFT+x", Some("safe\"\\\nvalue"))
        );
    }

    #[test]
    fn strict_registry_rejects_unknown_top_level_field() {
        let error =
            parse_native_config_document("return {\n    definitely_unknown = true,\n}", &[])
                .unwrap_err();

        assert!(matches!(
            error,
            NativeConfigLoadError::UnknownField {
                location: SourceLocation { line: 2, column: 5 },
                ref field,
            } if field == "definitely_unknown"
        ));
    }

    #[test]
    fn strict_registry_rejects_mixed_known_and_unknown_colors_keys() {
        let error = parse_native_config_document(
            r##"return {
                colors = {
                    cursor_bg = "#ffffff",
                    compose_cursor = "#123456",
                    unexpected_cursor_key = "#000000",
                },
            }"##,
            &[],
        )
        .unwrap_err();

        assert!(matches!(
            error,
            NativeConfigLoadError::InvalidFieldValue {
                ref field,
                ref message,
                ..
            } if field == "colors" && message.contains("unexpected_cursor_key")
        ));
    }

    #[test]
    fn strict_colors_accepts_compose_cursor_and_converts_it() {
        let overrides = parse_native_config_document(
            r##"return { colors = { compose_cursor = "#123456" } }"##,
            &[],
        )
        .unwrap();

        assert_eq!(
            overrides
                .colors
                .as_ref()
                .and_then(|colors| colors.compose_cursor),
            Some(rssh_terminal::Color::Rgb(0x12, 0x34, 0x56))
        );
    }

    #[test]
    fn strict_registry_rejects_mixed_valid_and_unsupported_key_entries() {
        let error = parse_native_config_document(
            r#"return {
                keys = {
                    {
                        key = "x",
                        mods = "CTRL",
                        action = wezterm.action.SendString("ok"),
                    },
                    { key = "y", mods = "ALT", action = { DynamicAction = "bad" } },
                },
            }"#,
            &[],
        )
        .unwrap_err();

        assert!(matches!(
            error,
            NativeConfigLoadError::UnsupportedDynamicLua { .. }
        ));
    }

    #[test]
    fn strict_keys_reject_noncanonical_send_string_action_forms() {
        for action in [
            r#"{ SendString = "x" }"#,
            r#"wezterm.action { SendString = "x" }"#,
            r#"wezterm.action.SendString "x""#,
        ] {
            let source = format!("return {{ keys = {{ {{ key = \"x\", action = {action} }} }} }}");
            assert!(matches!(
                parse_native_config_document(&source, &[]),
                Err(NativeConfigLoadError::UnsupportedDynamicLua { .. })
            ));
        }
    }

    #[test]
    fn strict_keys_reject_mods_and_mod_alias_duplicates_before_legacy() {
        let error = parse_native_config_document(
            r#"return {
                keys = {
                    {
                        key = "x",
                        mods = "CTRL",
                        mod = "SHIFT",
                        action = wezterm.action.SendString("x"),
                    },
                },
            }"#,
            &[],
        )
        .unwrap_err();

        assert!(matches!(
            error,
            NativeConfigLoadError::InvalidFieldValue {
                ref field,
                ref message,
                ..
            } if field == "keys" && message.contains("duplicate modifier")
        ));
    }

    #[test]
    fn strict_canonical_quotes_reserved_and_injection_like_environment_keys() {
        let source = r#"return {
                set_environment_variables = {
                    ["return"] = "reserved-return",
                    ["end"] = "reserved-end",
                    ["function"] = "reserved-function",
                    ["x\"] = true; injected = [\""] = "key-roundtrip",
                    ["VALUE"] = "\"]}; os.execute('never'); --",
                },
            }"#;
        let assignments = Parser::new(source).parse_document().unwrap();
        let canonical = canonical_document(&assignments);
        assert!(canonical.contains(r#"["return"]="reserved-return""#));
        assert!(canonical.contains(r#"["end"]="reserved-end""#));
        assert!(canonical.contains(r#"["function"]="reserved-function""#));

        let overrides = parse_native_config_document(source, &[]).unwrap();

        let environment = overrides.set_environment_variables.unwrap();
        assert_eq!(
            environment.get("return").map(String::as_str),
            Some("reserved-return")
        );
        assert_eq!(
            environment.get("end").map(String::as_str),
            Some("reserved-end")
        );
        assert_eq!(
            environment.get("function").map(String::as_str),
            Some("reserved-function")
        );
        assert_eq!(
            environment
                .get("x\"] = true; injected = [\"")
                .map(String::as_str),
            Some("key-roundtrip")
        );
        assert_eq!(
            environment.get("VALUE").map(String::as_str),
            Some("\"]}; os.execute('never'); --")
        );
    }

    #[test]
    fn strict_environment_action_key_identifier_form_roundtrips() {
        let source = r#"return {
                set_environment_variables = {
                    action = "literal",
                },
            }"#;

        let assignments = Parser::new(source).parse_document().unwrap();
        let canonical = canonical_document(&assignments);
        assert!(canonical.contains(r#"action="literal""#));

        let overrides = parse_native_config_document(source, &[]).unwrap();
        assert_eq!(
            overrides
                .set_environment_variables
                .as_ref()
                .and_then(|environment| environment.get("action"))
                .map(String::as_str),
            Some("literal")
        );
    }

    #[test]
    fn strict_environment_action_key_bracketed_form_roundtrips() {
        let source = r#"return {
                set_environment_variables = {
                    ["action"] = [=[long
literal]=],
                },
            }"#;

        let assignments = Parser::new(source).parse_document().unwrap();
        let canonical = canonical_document(&assignments);
        assert!(canonical.contains(r#"action="long\nliteral""#));

        let overrides = parse_native_config_document(source, &[]).unwrap();
        assert_eq!(
            overrides
                .set_environment_variables
                .as_ref()
                .and_then(|environment| environment.get("action"))
                .map(String::as_str),
            Some("long\nliteral")
        );
    }

    #[test]
    fn strict_registry_rejects_trailing_tokens_inside_composite_value() {
        let error = validate_cli_config_overrides(&[(
            "colors".to_owned(),
            "{ cursor_bg = '#ffffff' } trailing".to_owned(),
        )])
        .unwrap_err();

        assert!(matches!(
            error,
            NativeConfigLoadError::UnsupportedDynamicLua {
                location: SourceLocation { line: 1, .. },
                ..
            }
        ));
    }

    #[test]
    fn strict_cli_overrides_validate_and_last_duplicate_wins() {
        let items = vec![
            ("term".to_owned(), "'first'".to_owned()),
            ("enable_tab_bar".to_owned(), "false -- comment".to_owned()),
            ("term".to_owned(), r#""last\"safe""#.to_owned()),
        ];

        let cli = validate_cli_config_overrides(&items).unwrap();
        assert_eq!(cli.len(), 3);
        assert_eq!(cli[0].field_path, ["term"]);
        assert_eq!(cli[2].value_source, r#""last\"safe""#);
        assert_eq!(cli[2].location, SourceLocation { line: 1, column: 1 });

        let overrides =
            parse_native_config_document("return { term = 'from-file' }", &cli).unwrap();
        assert_eq!(overrides.term.as_deref(), Some("last\"safe"));
        assert_eq!(overrides.enable_tab_bar, Some(false));

        assert!(matches!(
            validate_cli_config_overrides(&[("unknown".to_owned(), "true".to_owned())]),
            Err(NativeConfigLoadError::UnknownField { .. })
        ));
        assert!(matches!(
            validate_cli_config_overrides(&[("initial_cols".to_owned(), "-1".to_owned())]),
            Err(NativeConfigLoadError::InvalidFieldValue { .. })
        ));
    }
}
