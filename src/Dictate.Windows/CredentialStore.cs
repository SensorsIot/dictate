using System.Runtime.InteropServices;
using System.Text;

namespace Dictate.Windows;

/// <summary>
/// API keys in Windows Credential Manager, DPAPI-encrypted under the logged-in
/// user. Chosen over a .env file so there is no plaintext key sitting on a
/// daily-driver machine, and over the Infisical CLI so dictation still works
/// off the home LAN.
/// </summary>
internal static class CredentialStore
{
    internal const string ElevenLabsTarget = "dictate:elevenlabs";
    internal const string AnthropicTarget = "dictate:anthropic";

    internal static string? Read(string target)
    {
        if (!Interop.CredRead(target, Interop.CRED_TYPE_GENERIC, 0, out var handle))
        {
            return null;
        }

        try
        {
            var credential = Marshal.PtrToStructure<Interop.CREDENTIALW>(handle);
            if (credential.CredentialBlobSize == 0 || credential.CredentialBlob == IntPtr.Zero)
            {
                return null;
            }

            var bytes = new byte[credential.CredentialBlobSize];
            Marshal.Copy(credential.CredentialBlob, bytes, 0, bytes.Length);
            return Encoding.UTF8.GetString(bytes);
        }
        finally
        {
            Interop.CredFree(handle);
        }
    }

    internal static void Write(string target, string secret)
    {
        var blob = Encoding.UTF8.GetBytes(secret);
        var blobPtr = Marshal.AllocHGlobal(blob.Length);
        var targetPtr = Marshal.StringToCoTaskMemUni(target);
        var userPtr = Marshal.StringToCoTaskMemUni(Environment.UserName);

        try
        {
            Marshal.Copy(blob, 0, blobPtr, blob.Length);

            var credential = new Interop.CREDENTIALW
            {
                Type = Interop.CRED_TYPE_GENERIC,
                TargetName = targetPtr,
                CredentialBlobSize = (uint)blob.Length,
                CredentialBlob = blobPtr,
                Persist = Interop.CRED_PERSIST_LOCAL_MACHINE,
                UserName = userPtr,
            };

            if (!Interop.CredWrite(ref credential, 0))
            {
                throw new InvalidOperationException(
                    $"Could not store '{target}' in Credential Manager (Win32 error {Marshal.GetLastWin32Error()}).");
            }
        }
        finally
        {
            // Zero the unmanaged copy before releasing it — a freed heap block
            // is not wiped, and this one held an API key.
            for (var i = 0; i < blob.Length; i++)
            {
                Marshal.WriteByte(blobPtr, i, 0);
            }

            Marshal.FreeHGlobal(blobPtr);
            Marshal.FreeCoTaskMem(targetPtr);
            Marshal.FreeCoTaskMem(userPtr);
            Array.Clear(blob);
        }
    }

    internal static void Delete(string target) =>
        Interop.CredDelete(target, Interop.CRED_TYPE_GENERIC, 0);
}
