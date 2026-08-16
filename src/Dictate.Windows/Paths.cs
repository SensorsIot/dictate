namespace Dictate.Windows;

internal static class Paths
{
    internal static string ConfigFolder =>
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "dictate");

    internal static string ConfigFile => Path.Combine(ConfigFolder, "config.json");

    /// <summary>
    /// Under LocalApplicationData rather than beside the config: a log is
    /// machine-local and must never follow a roaming profile onto other
    /// machines.
    /// </summary>
    internal static string LogFile => Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
        "dictate",
        "dictate.log");
}
