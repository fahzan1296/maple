# Appendix C — Platform Capability Matrix

The matrix is a planning template. Exact API behavior must be verified against supported OS versions.

| Capability | Linux | macOS | Windows | Required fallback |
|---|---|---|---|---|
| process memory hard limit | cgroups/rlimits depending setup | limited per-process controls | Job Objects | soft monitor + terminate |
| process tree containment | cgroups/process groups | process groups | Job Objects | supervisor tracking |
| file atomic replace | rename semantics | rename semantics | replace APIs | documented weaker mode |
| directory sync | generally available | platform-specific | different semantics | durability downgrade |
| file identity | inode/device | inode/device | file ID | digest fallback |
| terminal raw mode | termios | termios | console APIs | noninteractive mode |
| filesystem watcher | inotify | FSEvents/kqueue | ReadDirectoryChangesW | polling |
| credential store | optional desktop/keyring | Keychain | Credential Manager | env var/manual secret |
| resource sampling | procfs/cgroups | task/process APIs | process/job APIs | advisory reporting |

## Rules

1. Expose enforcement level, not just configured value.
2. Detect unsupported features at startup.
3. Do not silently substitute weaker behavior for security-critical controls.
4. Keep platform-specific code inside infrastructure adapters.
5. Test each supported target in CI or a documented release qualification environment.
