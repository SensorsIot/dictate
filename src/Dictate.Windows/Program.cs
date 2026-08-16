using Dictate.Core;

namespace Dictate.Windows;

internal static class Program
{
    [STAThread]
    private static int Main(string[] args)
    {
        ApplicationConfiguration.Initialize();

        if (args.Any(a => a is "--auth" or "-a"))
        {
            return AuthForm.Prompt() ? 0 : 1;
        }

        DictateConfig config;
        try
        {
            config = DictateConfig.Load(Paths.ConfigFile);
        }
        catch (Exception ex)
        {
            // FR-16.2: a broken config file stops startup. Falling back to
            // defaults would look exactly like the settings never applying.
            Fatal($"{Paths.ConfigFile} could not be read:\n\n{ex.Message}\n\n" +
                  "Fix or delete the file and start dictate again.");
            return 2;
        }

        var elevenLabsKey = CredentialStore.Read(CredentialStore.ElevenLabsTarget);
        if (string.IsNullOrWhiteSpace(elevenLabsKey))
        {
            if (!AuthForm.Prompt())
            {
                return 3;
            }

            elevenLabsKey = CredentialStore.Read(CredentialStore.ElevenLabsTarget);
            if (string.IsNullOrWhiteSpace(elevenLabsKey))
            {
                return 3;
            }
        }

        var http = new HttpClient
        {
            // Belt to the pipeline's braces: this bounds the socket, the
            // pipeline bounds the wait.
            Timeout = TimeSpan.FromSeconds(config.ScribeTimeoutSeconds + 5),
        };

        var transcriber = new ScribeTranscriber(http, elevenLabsKey, config.ScribeModelId);

        // FR-11.10: no Anthropic key is a degraded mode, not a failure — you
        // still get dictation, just verbatim.
        var anthropicKey = CredentialStore.Read(CredentialStore.AnthropicTarget);
        ICleaner cleaner = string.IsNullOrWhiteSpace(anthropicKey)
            ? new PassthroughCleaner()
            : new HaikuCleaner(anthropicKey, config);

        var pipeline = new DictationPipeline(transcriber, cleaner, config);

        try
        {
            using var app = new DictateApp(config, pipeline);
            Application.Run(app);
        }
        catch (Exception ex)
        {
            Fatal($"dictate could not start:\n\n{ex.Message}");
            return 4;
        }
        finally
        {
            http.Dispose();
        }

        return 0;
    }

    private static void Fatal(string message) =>
        MessageBox.Show(message, "dictate", MessageBoxButtons.OK, MessageBoxIcon.Error);
}
