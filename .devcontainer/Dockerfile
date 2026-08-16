FROM mcr.microsoft.com/devcontainers/base:debian

# Rename the base image's vscode user to dev, keeping UID 1000 so it matches the
# host user. Host-owned bind mounts are then readable directly and per-user npm
# needs no sudo.
RUN usermod -l dev -d /home/dev -m vscode \
    && groupmod -n dev vscode \
    && sed -i 's/vscode/dev/g' /etc/sudoers.d/vscode || true \
    && mv /etc/sudoers.d/vscode /etc/sudoers.d/dev 2>/dev/null || true

# Drop zsh/oh-my-zsh (~15MB, unused here)
RUN rm -rf /home/*/.oh-my-zsh /root/.oh-my-zsh \
    && apt-get purge -y zsh && apt-get autoremove -y

# Editing and git only. The .NET SDK is deliberately NOT installed — the
# solution is built and tested by GitHub Actions (.github/workflows/), by
# explicit choice. Consequences, so nobody rediscovers them the hard way:
#   * `dotnet build` / `dotnet test` do not exist here; failures surface on push.
#   * The C# language server (ms-dotnettools.csharp) needs the SDK, so VS Code
#     gives syntax highlighting but no IntelliSense in this container.
# To change that, add the dotnet feature to devcontainer.json and rebuild.
RUN apt-get update && apt-get install -y --no-install-recommends \
        tmux \
        jq \
        && rm -rf /var/lib/apt/lists/*

# Per-user npm prefix on PATH for every shell, interactive or not. Must be an
# image ENV: Debian's .bashrc returns early for non-interactive shells, so an
# export there would be invisible to `ssh <host> claude ...`, hooks and scripts.
ENV PATH="/home/dev/.npm-global/bin:${PATH}"

USER dev

# UTF-8 so tmux renders block glyphs correctly (claude's splash logo)
RUN echo 'export LANG=C.UTF-8' >> /home/dev/.bashrc

# Persistent claude session: interactive SSH logins attach to a tmux session
# "main" so work survives disconnects. Skips non-interactive shells, shells
# already inside tmux, VS Code terminals, and any login while ~/.no-tmux exists.
RUN printf '%s\n' \
    '' \
    '# --- Persistent claude session (auto-attach tmux on interactive SSH) ---' \
    'if [ -n "$SSH_CONNECTION" ] && [ -z "$TMUX" ] && [ "$TERM_PROGRAM" != "vscode" ] && [ ! -f "$HOME/.no-tmux" ] && command -v tmux >/dev/null 2>&1; then' \
    '    case "$-" in' \
    '        *i*) exec tmux new-session -A -s main -c /workspaces/dictate "claude --dangerously-skip-permissions; exec bash" ;;' \
    '    esac' \
    'fi' \
    >> /home/dev/.bashrc

WORKDIR /workspaces/dictate
RUN echo 'cd /workspaces/dictate 2>/dev/null' >> /home/dev/.bashrc
