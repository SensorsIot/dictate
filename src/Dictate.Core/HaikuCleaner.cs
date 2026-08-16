using Anthropic;
using Anthropic.Models.Messages;

namespace Dictate.Core;

public interface ICleaner
{
    Task<string> CleanAsync(string transcript, TargetContext target, CancellationToken ct);
}

/// <summary>
/// Cleanup via Claude Haiku 4.5 — the tier that is cheap and fast enough to sit
/// in a push-to-talk loop.
/// </summary>
public sealed class HaikuCleaner : ICleaner
{
    private readonly AnthropicClient _client;
    private readonly DictateConfig _config;
    private readonly string _systemPrompt;

    public HaikuCleaner(string apiKey, DictateConfig config)
    {
        _client = new AnthropicClient { ApiKey = apiKey };
        _config = config;

        // Built once: it is identical for every utterance.
        //
        // No cache_control here on purpose. Haiku 4.5's minimum cacheable prefix
        // is 4096 tokens and this prompt is a few hundred, so a breakpoint would
        // be silently ignored — marking it would only imply a saving that never
        // arrives. Revisit if the vocabulary ever grows past that threshold.
        _systemPrompt = CleanupPrompt.BuildSystem(config);
    }

    public async Task<string> CleanAsync(string transcript, TargetContext target, CancellationToken ct)
    {
        var parameters = new MessageCreateParams
        {
            Model = _config.CleanupModel,

            // Cleanup only ever shortens the transcript. Generous headroom for
            // German compounds, still far below anything that could run long.
            MaxTokens = 2048,

            // Deterministic: the same utterance should not clean up two ways.
            Temperature = 0,

            System = _systemPrompt,
            Messages =
            [
                new() { Role = Role.User, Content = CleanupPrompt.BuildUser(transcript, target) },
            ],
        };

        // WaitAsync rather than the SDK's own timeout knob: this is plain BCL and
        // cannot drift with the client surface. It stops us waiting; the request
        // itself is abandoned rather than cancelled, which is fine for one small
        // call that the caller is about to give up on anyway.
        var response = await _client.Messages
            .Create(parameters)
            .WaitAsync(TimeSpan.FromSeconds(_config.CleanupTimeoutSeconds), ct);

        var text = string.Concat(
            response.Content
                .Select(block => block.Value)
                .OfType<TextBlock>()
                .Select(block => block.Text));

        return text.Trim();
    }
}

/// <summary>
/// Cleanup that does nothing. Used when no Anthropic key is configured, so the
/// tool still dictates — just verbatim.
/// </summary>
public sealed class PassthroughCleaner : ICleaner
{
    public Task<string> CleanAsync(string transcript, TargetContext target, CancellationToken ct) =>
        Task.FromResult(transcript);
}
