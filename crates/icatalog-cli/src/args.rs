use std::path::PathBuf;

use icatalog_engine::Params;

/// A parsed command-line invocation.
pub struct Invocation {
    pub command: String,
    pub catalogue: PathBuf,
    pub params: Params,
}

/// Parse the tokens after the command name. `args[0]` is the catalogue path;
/// `--name value` sets a parameter; extra positionals are rejected except for
/// `find` (query) and `collection` (action), which require one positional.
pub fn parse(command: &str, args: &[String]) -> std::result::Result<Invocation, String> {
    let Some(catalogue) = args.first() else {
        return Err("missing <catalogue> path".to_owned());
    };
    if catalogue.starts_with('-') {
        return Err(format!(
            "missing <catalogue> path (got option '{catalogue}')"
        ));
    }
    let mut params = Params::new();
    let mut positionals: Vec<&String> = Vec::new();
    let mut iter = args.iter().skip(1);
    while let Some(tok) = iter.next() {
        if tok.starts_with("--") && tok.len() > 2 {
            let name = tok.trim_start_matches("--");
            let Some(value) = iter.next() else {
                return Err(format!("option '{tok}' needs a value"));
            };
            params.set(name, value);
        } else if tok.starts_with('-') {
            return Err(format!("unknown option '{tok}'"));
        } else {
            positionals.push(tok);
        }
    }
    if command == "find" || command == "collection" {
        let key = if command == "find" { "query" } else { "action" };
        let mut it = positionals.into_iter();
        let Some(query) = it.next() else {
            return Err(format!("{command} requires a <{key}> argument"));
        };
        if let Some(extra) = it.next() {
            return Err(format!("unexpected argument '{extra}'"));
        }
        params.set(key, query);
    } else if let Some(p) = positionals.first() {
        return Err(format!("unexpected argument '{p}'"));
    }
    Ok(Invocation {
        command: command.to_owned(),
        catalogue: PathBuf::from(catalogue),
        params,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn catalogue_only() {
        let i = parse("list", &v(&["c.sqlite"])).unwrap();
        assert_eq!(i.catalogue, PathBuf::from("c.sqlite"));
        assert!(i.params.is_empty());
    }

    #[test]
    fn limit_option() {
        let i = parse("list", &v(&["c", "--limit", "5"])).unwrap();
        assert_eq!(i.params.get("limit"), Some("5"));
    }

    #[test]
    fn find_with_query() {
        let i = parse("find", &v(&["c", "cat"])).unwrap();
        assert_eq!(i.params.get("query"), Some("cat"));
    }

    #[test]
    fn find_without_query() {
        assert!(parse("find", &v(&["c"])).is_err());
    }

    #[test]
    fn unknown_option() {
        assert!(parse("list", &v(&["c", "-x"])).is_err());
    }

    #[test]
    fn option_missing_value() {
        assert!(parse("list", &v(&["c", "--limit"])).is_err());
    }

    #[test]
    fn extra_positional() {
        assert!(parse("list", &v(&["c", "extra"])).is_err());
        assert!(parse("find", &v(&["c", "a", "b"])).is_err());
        assert!(parse("list", &[]).is_err());
    }
}
