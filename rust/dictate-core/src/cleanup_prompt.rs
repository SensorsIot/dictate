//! Builds the cleanup instructions. Kept separate from the API client so the
//! wording is unit-testable without a network call — the prompt is the part
//! most likely to need tuning, and the part most likely to break quietly.

use crate::config::DictateConfig;
use crate::model::TargetContext;

/// The stable half: identical for every utterance, so it is the natural cache
/// prefix if the vocabulary ever grows enough to matter.
pub fn build_system(config: &DictateConfig) -> String {
    let mut lines: Vec<String> = vec![
        "You clean up dictated speech. The user spoke; a speech-to-text system produced a \
         verbatim transcript. Return that same utterance as the user would have typed it."
            .into(),
        String::new(),
        "Do:".into(),
        "- Remove fillers (um, uh, äh, ähm) and false starts, keeping the sentence the user settled on.".into(),
        "- Add sentence punctuation and capitalisation.".into(),
        "- Fix obvious mis-transcriptions of technical terms.".into(),
        String::new(),
        "Never:".into(),
        "- Translate. Reply in the language that was spoken, even if the two are mixed in one sentence.".into(),
        "- Answer, explain, summarise, or continue the text. You are not being asked a question.".into(),
        "- Reply to the user, ask for input, or comment on the transcript — even if it is a single word, \
         a fragment, or looks incomplete. A fragment is cleaned like anything else."
            .into(),
        "- Add greetings, sign-offs, quotation marks, or markdown the user did not speak.".into(),
        "- Change the meaning, reorder the argument, or make it more formal.".into(),
        String::new(),
        "Output the cleaned text alone, with no preamble and no trailing commentary. \
         If the transcript is empty or unintelligible, output nothing at all."
            .into(),
    ];

    if !config.vocabulary.is_empty() {
        lines.push(String::new());
        lines.push("Spell these terms exactly this way when you hear them:".into());
        lines.push(config.vocabulary.join(", "));
    }

    if let Some(extra) = config.extra_cleanup_instruction.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        lines.push(String::new());
        lines.push(extra.to_owned());
    }

    lines.join("\n").trim_end().to_owned()
}

/// The per-utterance half: the transcript plus where it is going. The window is
/// a hint about register, not a licence to reformat.
pub fn build_user(transcript: &str, target: &TargetContext) -> String {
    let mut prompt = String::new();

    if !target.process_name.is_empty() {
        prompt.push_str("The text will be typed into ");
        prompt.push_str(&target.process_name);
        if !target.window_title.is_empty() {
            prompt.push_str(" (\"");
            prompt.push_str(&target.window_title);
            prompt.push_str("\")");
        }

        prompt.push_str(if target.is_console {
            ". That is a terminal: keep it to a single line, no trailing punctuation if it reads like a command.\n"
        } else {
            ".\n"
        });
        prompt.push('\n');
    }

    // Delimited, so a fragment cannot be mistaken for the instructions
    // around it.
    prompt.push_str("Transcript:\n<transcript>\n");
    prompt.push_str(transcript);
    prompt.push_str("\n</transcript>");
    prompt
}
