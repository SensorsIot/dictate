namespace Dictate.Windows;

internal static class Paths
{
    internal static string ConfigFolder =>
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "dictate");

    internal static string ConfigFile => Path.Combine(ConfigFolder, "config.json");
}
