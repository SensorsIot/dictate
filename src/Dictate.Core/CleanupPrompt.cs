using System.Text;

namespace Dictate.Core;

/// <summary>
/// Builds the cleanup instructions. Kept separate from the API client so the
/// wording is unit-testable without a network call — the prompt is the part
/// most likely to need tuning, and the part most likely to break quietly.
/// </summary>
public static class CleanupPrompt
{
    /// <summary>
    /// The stable half: identical for every utterance, so it is the natural
    /// cache prefix if the vocabulary ever grows enough to matter.
    /// </summary>
    public static string BuildSystem(DictateConfig config)
    {
        var sb = new StringBuilder();

        sb.AppendLine(
            "You clean up dictated speech. The user spoke; a speech-to-text system produced a " +
            "verbatim transcript. Return that same utterance as the user would have typed it.");
        sb.AppendLine();

        sb.AppendLine("Do:");
        sb.AppendLine("- Remove fillers (um, uh, äh, ähm) and false starts, keeping the sentence the user settled on.");
        sb.AppendLine("- Add sentence punctuation and capitalisation.");
        sb.AppendLine("- Fix obvious mis-transcriptions of technical terms.");
        sb.AppendLine();

        sb.AppendLine("Never:");
        sb.AppendLine("- Translate. Reply in the language that was spoken, even if the two are mixed in one sentence.");
        sb.AppendLine("- Answer, explain, summarise, or continue the text. You are not being asked a question.");
        sb.AppendLine("- Add greetings, sign-offs, quotation marks, or markdown the user did not speak.");
        sb.AppendLine("- Change the meaning, reorder the argument, or make it more formal.");
        sb.AppendLine();

        sb.AppendLine("Output the cleaned text alone, with no preamble and no trailing commentary. " +
                      "If the transcript is empty or unintelligible, output nothing at all.");

        if (config.Vocabulary.Count > 0)
        {
            sb.AppendLine();
            sb.AppendLine("Spell these terms exactly this way when you hear them:");
            sb.AppendLine(string.Join(", ", config.Vocabulary));
        }

        if (!string.IsNullOrWhiteSpace(config.ExtraCleanupInstruction))
        {
            sb.AppendLine();
            sb.AppendLine(config.ExtraCleanupInstruction.Trim());
        }

        return sb.ToString().TrimEnd();
    }

    /// <summary>
    /// The per-utterance half: the transcript plus where it is going. The window
    /// is a hint about register, not a licence to reformat.
    /// </summary>
    public static string BuildUser(string transcript, TargetContext target)
    {
        var sb = new StringBuilder();

        if (target.ProcessName.Length > 0)
        {
            sb.Append("The text will be typed into ").Append(target.ProcessName);
            if (target.WindowTitle.Length > 0)
            {
                sb.Append(" (\"").Append(target.WindowTitle).Append("\")");
            }

            sb.AppendLine(target.IsConsole
                ? ". That is a terminal: keep it to a single line, no trailing punctuation if it reads like a command."
                : ".");
            sb.AppendLine();
        }

        sb.AppendLine("Transcript:");
        sb.Append(transcript);

        return sb.ToString();
    }
}
