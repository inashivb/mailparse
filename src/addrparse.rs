/// A representation of a single mailbox. Each mailbox has
/// a routing address `addr` and an optional display name.
#[derive(Debug, PartialEq)]
pub struct SingleInfo {
    pub display_name: Option<String>,
    pub addr: String,
}

impl SingleInfo {
    fn new(name: Option<String>, addr: String) -> Self {
        SingleInfo {
            display_name: name,
            addr: addr,
        }
    }
}

/// A representation of a group address. It has a name and
/// a list of mailboxes.
#[derive(Debug, PartialEq)]
pub struct GroupInfo {
    pub group_name: String,
    pub addrs: Vec<SingleInfo>,
}

impl GroupInfo {
    fn new(name: String, addrs: Vec<SingleInfo>) -> Self {
        GroupInfo {
            group_name: name,
            addrs: addrs,
        }
    }
}

/// An abstraction over the two different kinds of top-level addresses allowed
/// in email headers. Group addresses have a name and a list of mailboxes. Single
/// addresses are just a mailbox. Each mailbox consists of what you would consider
/// an email address (e.g. foo@bar.com) and optionally a display name ("Foo Bar").
/// Groups are represented in email headers with colons and semicolons, e.g.
///    To: my-peeps: foo@peeps.org, bar@peeps.org;
#[derive(Debug, PartialEq)]
pub enum MailAddr {
    Group(GroupInfo),
    Single(SingleInfo),
}

#[derive(Debug)]
enum AddrParseState {
    Initial,
    QuotedName,
    EscapedChar,
    AfterQuotedName,
    BracketedAddr,
    AfterBracketedAddr,
    Unquoted,
    TrailerComment,
}

/// Convert an address field from an email header into a structured type.
/// This function handles the most common formatting of to/from/cc/bcc fields
/// found in email headers.
///
/// # Examples
/// ```
///     use mailparse::{addrparse, MailAddr, SingleInfo};
///     match &addrparse("John Doe <john@doe.com>").unwrap()[0] {
///         MailAddr::Single(info) => {
///             assert_eq!(info.display_name, Some("John Doe".to_string()));
///             assert_eq!(info.addr, "john@doe.com".to_string());
///         }
///         _ => panic!()
///     };
/// ```
pub fn addrparse(addrs: &str) -> Result<Vec<MailAddr>, &'static str> {
    let mut it = addrs.chars();
    addrparse_inner(&mut it, false)
}

fn addrparse_inner(it: &mut std::str::Chars, in_group: bool) -> Result<Vec<MailAddr>, &'static str> {
    let mut result = vec![];
    let mut state = AddrParseState::Initial;

    let mut c = match it.next() {
        None => return Ok(vec![]),
        Some(v) => v,
    };

    let mut name = None;
    let mut addr = None;
    let mut post_quote_ws = None;

    loop {
        match state {
            AddrParseState::Initial => {
                if c.is_whitespace() {
                    // continue in same state
                } else if c == '"' {
                    state = AddrParseState::QuotedName;
                    name = Some(String::new());
                } else if c == '<' {
                    state = AddrParseState::BracketedAddr;
                    addr = Some(String::new());
                } else if c == ';' {
                    if !in_group {
                        return Err("Unexpected group terminator found in initial list");
                    }
                    return Ok(result);
                } else {
                    state = AddrParseState::Unquoted;
                    addr = Some(String::new());
                    addr.as_mut().unwrap().push(c);
                }
            }
            AddrParseState::QuotedName => {
                if c == '\\' {
                    state = AddrParseState::EscapedChar;
                } else if c == '"' {
                    state = AddrParseState::AfterQuotedName;
                } else {
                    name.as_mut().unwrap().push(c);
                }
            }
            AddrParseState::EscapedChar => {
                state = AddrParseState::QuotedName;
                name.as_mut().unwrap().push(c);
            }
            AddrParseState::AfterQuotedName => {
                if c.is_whitespace() {
                    if post_quote_ws.is_none() {
                        post_quote_ws = Some(String::new());
                    }
                    post_quote_ws.as_mut().unwrap().push(c);
                } else if c == '<' {
                    state = AddrParseState::BracketedAddr;
                    addr = Some(String::new());
                } else if c == ':' {
                    if in_group {
                        return Err("Found unexpected nested group");
                    }
                    let group_addrs = try!(addrparse_inner(it, true));
                    state = AddrParseState::Initial;
                    result.push(MailAddr::Group(GroupInfo::new(
                        name.unwrap(),
                        group_addrs.into_iter().map(|addr| {
                            match addr {
                                MailAddr::Single(s) => s,
                                MailAddr::Group(_) => panic!("Unexpected nested group encountered"),
                            }
                        }).collect()
                    )));
                    name = None;
                } else {
                    // I think technically not valid, but this occurs in real-world corpus, so
                    // handle gracefully
                    if c == '"' {
                        post_quote_ws.map(|ws| name.as_mut().unwrap().push_str(&ws));
                        state = AddrParseState::QuotedName;
                    } else {
                        post_quote_ws.map(|ws| name.as_mut().unwrap().push_str(&ws));
                        name.as_mut().unwrap().push(c);
                    }
                    post_quote_ws = None;
                }
            }
            AddrParseState::BracketedAddr => {
                if c == '>' {
                    state = AddrParseState::AfterBracketedAddr;
                    result.push(MailAddr::Single(SingleInfo::new(name, addr.unwrap())));
                    name = None;
                    addr = None;
                } else {
                    addr.as_mut().unwrap().push(c);
                }
            }
            AddrParseState::AfterBracketedAddr => {
                if c.is_whitespace() {
                    // continue in same state
                } else if c == ',' {
                    state = AddrParseState::Initial;
                } else if c == ';' {
                    if in_group {
                        return Ok(result);
                    }
                    // Technically not valid, but a similar case occurs in real-world corpus, so handle it gracefully
                    state = AddrParseState::Initial;
                } else if c == '(' {
                    state = AddrParseState::TrailerComment;
                } else {
                    return Err("Unexpected char found after bracketed address");
                }
            }
            AddrParseState::Unquoted => {
                if c == '<' {
                    state = AddrParseState::BracketedAddr;
                    name = addr.map(|s| s.trim_end().to_owned());
                    addr = Some(String::new());
                } else if c == ',' {
                    state = AddrParseState::Initial;
                    result.push(MailAddr::Single(SingleInfo::new(None, addr.unwrap().trim_end().to_owned())));
                    addr = None;
                } else if c == ';' {
                    result.push(MailAddr::Single(SingleInfo::new(None, addr.unwrap().trim_end().to_owned())));
                    if in_group {
                        return Ok(result);
                    }
                    // Technically not valid, but occurs in real-world corpus, so handle it gracefully
                    state = AddrParseState::Initial;
                    addr = None;
                } else if c == ':' {
                    if in_group {
                        return Err("Found unexpected nested group");
                    }
                    let group_addrs = try!(addrparse_inner(it, true));
                    state = AddrParseState::Initial;
                    result.push(MailAddr::Group(GroupInfo::new(
                        addr.unwrap().trim_end().to_owned(),
                        group_addrs.into_iter().map(|addr| {
                            match addr {
                                MailAddr::Single(s) => s,
                                MailAddr::Group(_) => panic!("Unexpected nested group encountered"),
                            }
                        }).collect()
                    )));
                    addr = None;
                } else {
                    addr.as_mut().unwrap().push(c);
                }
            }
            AddrParseState::TrailerComment => {
                if c == ')' {
                    state = AddrParseState::AfterBracketedAddr;
                }
            }
        }

        c = match it.next() {
            None => break,
            Some(v) => v,
        };
    }

    if in_group {
        return Err("Found unterminated group address");
    }

    match state {
        AddrParseState::QuotedName |
        AddrParseState::EscapedChar |
        AddrParseState::AfterQuotedName |
        AddrParseState::BracketedAddr |
        AddrParseState::TrailerComment => {
            Err("Address string unexpected terminated")
        }
        AddrParseState::Unquoted => {
            result.push(MailAddr::Single(SingleInfo::new(None, addr.unwrap().trim_end().to_owned())));
            Ok(result)
        }
        _ => {
            Ok(result)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic() {
        assert_eq!(
            addrparse("foo bar <foo@bar.com>").unwrap(),
            vec![MailAddr::Single(SingleInfo::new(Some("foo bar".to_string()), "foo@bar.com".to_string()))]
        );
        assert_eq!(
            addrparse("\"foo bar\" <foo@bar.com>").unwrap(),
            vec![MailAddr::Single(SingleInfo::new(Some("foo bar".to_string()), "foo@bar.com".to_string()))]
        );
        assert_eq!(
            addrparse("foo@bar.com ").unwrap(),
            vec![MailAddr::Single(SingleInfo::new(None, "foo@bar.com".to_string()))]
        );
        assert_eq!(
            addrparse("foo <bar>").unwrap(),
            vec![MailAddr::Single(SingleInfo::new(Some("foo".to_string()), "bar".to_string()))]
        );
        assert_eq!(
            addrparse("\"foo\" <bar>").unwrap(),
            vec![MailAddr::Single(SingleInfo::new(Some("foo".to_string()), "bar".to_string()))]
        );
        assert_eq!(
            addrparse("\"foo \" <bar>").unwrap(),
            vec![MailAddr::Single(SingleInfo::new(Some("foo ".to_string()), "bar".to_string()))]
        );
    }

    #[test]
    fn parse_backslashes() {
        assert_eq!(
            addrparse(r#" "First \"nick\" Last" <user@host.tld> "#).unwrap(),
            vec![MailAddr::Single(SingleInfo::new(Some("First \"nick\" Last".to_string()), "user@host.tld".to_string()))]
        );
        assert_eq!(
            addrparse(r#" First \"nick\" Last <user@host.tld> "#).unwrap(),
            vec![MailAddr::Single(SingleInfo::new(Some("First \\\"nick\\\" Last".to_string()), "user@host.tld".to_string()))]
        );
    }

    #[test]
    fn parse_multi() {
        assert_eq!(
            addrparse("foo <bar>, joe, baz <quux>").unwrap(),
            vec![
                MailAddr::Single(SingleInfo::new(Some("foo".to_string()), "bar".to_string())),
                MailAddr::Single(SingleInfo::new(None, "joe".to_string())),
                MailAddr::Single(SingleInfo::new(Some("baz".to_string()), "quux".to_string())),
            ]
        );
    }

    #[test]
    fn parse_empty_group() {
        assert_eq!(
            addrparse("empty-group:;").unwrap(),
            vec![MailAddr::Group(GroupInfo::new("empty-group".to_string(), vec![]))]
        );
        assert_eq!(
            addrparse(" empty-group : ; ").unwrap(),
            vec![MailAddr::Group(GroupInfo::new("empty-group".to_string(), vec![]))]
        );
    }

    #[test]
    fn parse_simple_group() {
        assert_eq!(
            addrparse("bar-group: foo <foo@bar.com>;").unwrap(),
            vec![MailAddr::Group(GroupInfo::new("bar-group".to_string(), vec![
                SingleInfo::new(Some("foo".to_string()), "foo@bar.com".to_string()),
            ]))]
        );
        assert_eq!(
            addrparse("bar-group: foo <foo@bar.com>, baz@bar.com;").unwrap(),
            vec![MailAddr::Group(GroupInfo::new("bar-group".to_string(), vec![
                SingleInfo::new(Some("foo".to_string()), "foo@bar.com".to_string()),
                SingleInfo::new(None, "baz@bar.com".to_string()),
            ]))]
        );
    }

    #[test]
    fn parse_mixed() {
        assert_eq!(
            addrparse("joe@bloe.com, bar-group: foo <foo@bar.com>;").unwrap(),
            vec![
                MailAddr::Single(SingleInfo::new(None, "joe@bloe.com".to_string())),
                MailAddr::Group(GroupInfo::new("bar-group".to_string(), vec![
                    SingleInfo::new(Some("foo".to_string()), "foo@bar.com".to_string()),
                ])),
            ]
        );
        assert_eq!(
            addrparse("bar-group: foo <foo@bar.com>; joe@bloe.com").unwrap(),
            vec![
                MailAddr::Group(GroupInfo::new("bar-group".to_string(), vec![
                    SingleInfo::new(Some("foo".to_string()), "foo@bar.com".to_string()),
                ])),
                MailAddr::Single(SingleInfo::new(None, "joe@bloe.com".to_string())),
            ]
        );
        assert_eq!(
            addrparse("flim@flam.com, bar-group: foo <foo@bar.com>; joe@bloe.com").unwrap(),
            vec![
                MailAddr::Single(SingleInfo::new(None, "flim@flam.com".to_string())),
                MailAddr::Group(GroupInfo::new("bar-group".to_string(), vec![
                    SingleInfo::new(Some("foo".to_string()), "foo@bar.com".to_string()),
                ])),
                MailAddr::Single(SingleInfo::new(None, "joe@bloe.com".to_string())),
            ]
        );
        assert_eq!(
            addrparse("first-group:; flim@flam.com, bar-group: foo <foo@bar.com>; joe@bloe.com, final-group: zip, zap, \"Zaphod\" <zaphod@beeblebrox>;").unwrap(),
            vec![
                MailAddr::Group(GroupInfo::new("first-group".to_string(), vec![])),
                MailAddr::Single(SingleInfo::new(None, "flim@flam.com".to_string())),
                MailAddr::Group(GroupInfo::new("bar-group".to_string(), vec![
                    SingleInfo::new(Some("foo".to_string()), "foo@bar.com".to_string()),
                ])),
                MailAddr::Single(SingleInfo::new(None, "joe@bloe.com".to_string())),
                MailAddr::Group(GroupInfo::new("final-group".to_string(), vec![
                    SingleInfo::new(None, "zip".to_string()),
                    SingleInfo::new(None, "zap".to_string()),
                    SingleInfo::new(Some("Zaphod".to_string()), "zaphod@beeblebrox".to_string()),
                ])),
            ]
        );
    }

    #[test]
    fn real_world_examples() {
        // taken from a real "From" header. This might not be valid according to the RFC
        // but obviously made it through the internet so we should at least not crash.
        assert_eq!(
            addrparse("\"The Foo of Bar\" Course Staff <foo-no-reply@bar.edx.org>").unwrap(),
            vec![MailAddr::Single(SingleInfo::new(Some("The Foo of Bar Course Staff".to_string()), "foo-no-reply@bar.edx.org".to_string()))]
        );

        // This one has a comment tacked on to the end. Adding proper support for comments seems
        // complicated so I just added trailer comment support.
        assert_eq!(
            addrparse("John Doe <support@github.com> (GitHub Staff)").unwrap(),
            vec![MailAddr::Single(SingleInfo::new(Some("John Doe".to_string()), "support@github.com".to_string()))]
        );

        // Taken from a real world "To" header. It was spam, but still...
        assert_eq!(
            addrparse("foo@bar.com;").unwrap(),
            vec![MailAddr::Single(SingleInfo::new(None, "foo@bar.com".to_string()))]
        );
    }
}