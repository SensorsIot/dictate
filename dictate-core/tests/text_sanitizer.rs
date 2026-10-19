use dictate_core::text_sanitizer::{for_injection, looks_like_console, to_swiss_spelling, with_trailing_space};
use dictate_core::TargetContext;

fn console() -> TargetContext {
    TargetContext::new("wezterm-gui", "", true)
}

fn editor() -> TargetContext {
    TargetContext::new("Code", "main.rs", false)
}

#[test]
fn console_target_collapses_every_newline_to_a_space() {
    // The whole point: a newline typed into a shell is Enter, and Enter runs
    // whatever came before it.
    let result = for_injection("git status\nrm -rf /tmp/x", &console());

    assert!(!result.contains('\n'));
    assert!(!result.contains('\r'));
    assert_eq!(result, "git status rm -rf /tmp/x");
}

#[test]
fn console_target_handles_every_line_ending_flavour() {
    for input in ["one\r\ntwo", "one\rtwo", "one\ntwo"] {
        assert_eq!(for_injection(input, &console()), "one two", "input {input:?}");
    }
}

#[test]
fn non_console_target_keeps_paragraphs_as_crlf() {
    assert_eq!(for_injection("First line.\nSecond line.", &editor()), "First line.\r\nSecond line.");
}

#[test]
fn runs_of_blank_lines_are_capped_at_one() {
    assert_eq!(for_injection("a\n\n\n\nb", &editor()), "a\r\n\r\nb");
}

#[test]
fn surrounding_whitespace_is_trimmed() {
    assert_eq!(for_injection("  \n hello \n  ", &editor()), "hello");
}

#[test]
fn empty_text_stays_empty() {
    assert_eq!(for_injection("", &editor()), "");
}

#[test]
fn a_separator_is_added_so_consecutive_dictations_do_not_run_together() {
    assert_eq!(with_trailing_space("run together."), "run together. ");
}

#[test]
fn the_separator_applies_in_consoles_too() {
    // The original bug: the space was suppressed for console targets out of
    // misplaced caution. A newline in a shell presses Enter; a space does
    // nothing — and the terminal is where dictation is used most.
    let text = for_injection("git status", &console());

    assert_eq!(with_trailing_space(&text), "git status ");
}

#[test]
fn no_separator_is_added_to_nothing() {
    assert_eq!(with_trailing_space(""), "");
}

#[test]
fn known_terminals_are_recognised_regardless_of_case() {
    for process in ["wezterm-gui", "cmd", "powershell", "WindowsTerminal"] {
        assert!(looks_like_console(process, &[]), "{process}");
    }
}

#[test]
fn ordinary_apps_are_not_consoles() {
    for process in ["Code", "olk", ""] {
        assert!(!looks_like_console(process, &[]), "{process:?}");
    }
}

#[test]
fn extension_is_ignored_when_matching() {
    assert!(looks_like_console("cmd.exe", &[]));
}

#[test]
fn user_supplied_console_processes_are_honoured() {
    assert!(looks_like_console("myshell", &["myshell".to_owned()]));
    assert!(!looks_like_console("myshell", &[]));
}

#[test]
fn swiss_spelling_replaces_every_sharp_s() {
    for (input, expected) in [
        ("Die Straße ist groß.", "Die Strasse ist gross."),
        ("GROẞ", "GROSS"),
        ("Strasse", "Strasse"),
        ("plain English", "plain English"),
    ] {
        let result = to_swiss_spelling(input);

        assert_eq!(result, expected);
        assert!(!result.contains('ß'));
        assert!(!result.contains('ẞ'));
    }
}
