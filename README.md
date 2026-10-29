<p align="center">
  <picture>
    <img alt="virtx" src="assets/virtx-logo.png" width="352" style="max-width: 50%;">
  </picture>
</p>

<h3 align="center">Disposable Linux VMs for your code and your AI agents.</h3>

<p align="center">
  <img src="https://cdn.simpleicons.org/rust/000000/ffffff" width="16"/> <a href="https://crates.io/crates/virtx"><img src="https://img.shields.io/crates/v/virtx?label=virtx&color=dea584" alt="crates.io"></a>
  <img src="https://cdn.simpleicons.org/python" width="16"/> <a href="https://pypi.org/project/virtx/"><img src="https://img.shields.io/pypi/v/virtx?color=blue&label=virtx" alt="PyPI"></a>
  <img src="https://cdn.simpleicons.org/nodedotjs" width="16"/> <a href="https://www.npmjs.com/package/@brekkylab/virtx"><img src="https://img.shields.io/npm/v/@brekkylab/virtx?label=@brekkylab/virtx&color=339933" alt="npm node"></a>
</p>

<p align="center">
  <a href="https://discord.gg/27rx3EJy3P"><img src="https://img.shields.io/badge/Discord-7289DA?logo=discord&logoColor=white" alt="Discord"></a>
  <a href="https://x.com/brekkylab"><img src="https://img.shields.io/badge/X-000000?logo=x&logoColor=white" alt="X"></a>
</p>

<br>

virtx starts a Linux VM from your own code, runs what you ask inside it, and throws it away when you're done. The VM only sees the folders you mount, so a script, a build or an AI agent can install and change whatever it likes without touching your machine.

There is no Docker and no VM daemon to set up. Your program starts the VM, uses it and closes it, on Linux, macOS and Windows, and the VM can use your GPU through Vulkan when a task needs one.

virtx is also the VM layer under [Ailoy](https://github.com/brekkylab/ailoy), our open-source agent SDK. In Ailoy, every agent gets its own virtx VM, which is both the sandbox its commands run in and the machine the agent lives on. You can use virtx on its own too, with any agent or none at all.

## Quickstart

### Python

```sh
pip install virtx
```

```python
import asyncio

from virtx import ConsoleClient, Recipe, ensure_virtx


async def main() -> None:
    # Fetches the console server into virtx's cache the first time; a no-op after.
    await ensure_virtx()
    console = await (
        ConsoleClient.builder()
        .image(Recipe("alpine:latest").step("apk add --no-cache jq"))
        .build()
    )

    async with console:
        result = await console.exec(
            ["sh", "-c", """echo '{"hello": "virtx"}' | jq -r .hello"""]
        )

    print("\n" + result.stdout.decode(), end="")


asyncio.run(main())
```

### Node

```sh
npm install @brekkylab/virtx
```

```js
import { ConsoleClient, Recipe, ensureVirtx } from '@brekkylab/virtx'

// Fetches the console server into virtx's cache the first time; a no-op after.
await ensureVirtx()
const console_ = await ConsoleClient.builder()
  .image(new Recipe('alpine:latest').step('apk add --no-cache jq'))
  .build()

try {
  const result = await console_.exec(['sh', '-c', `echo '{"hello": "virtx"}' | jq -r .hello`])
  process.stdout.write('\n' + result.stdout)
} finally {
  await console_.close()
}
```

### Rust

```sh
cargo add virtx
```

```rust
use virtx::{console::ConsoleClient, ensure_virtx, image::Recipe};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    ensure_virtx().await?;
    let mut console = ConsoleClient::builder()
        .image(Recipe::new("alpine:latest").step("apk add --no-cache jq"))
        .build()
        .await?;

    let result = console
        .exec(["sh", "-c", r#"echo '{"hello": "virtx"}' | jq -r .hello"#], None)
        .await?;

    print!("\n{}", String::from_utf8_lossy(&result.stdout));

    Ok(())
}
```

It prints:
```text
step 1/1: cd '/' && apk add --no-cache jq
(1/2) Installing oniguruma (6.9.10-r0)
(2/2) Installing jq (1.8.2-r0)
Executing busybox-1.37.0-r31.trigger
OK: 9426 KiB in 18 packages

virtx
```

Or let a coding agent do it: this repo is its own plugin marketplace, so the [agent skill](skills/virtx/SKILL.md) installs with no clone first.

```bash
claude plugin marketplace add brekkylab/virtx && claude plugin install virtx@brekkylab
codex plugin marketplace add brekkylab/virtx && codex plugin add virtx@brekkylab
```

## Features

### Coverage

| | Supported |
|---|---|
| **Languages** | <img src="https://cdn.simpleicons.org/python" width="16"/> Python · <img src="https://cdn.simpleicons.org/nodedotjs" width="16"/> Node · <img src="https://cdn.simpleicons.org/rust/000000/ffffff" width="16"/> Rust |
| **Hosts** | <img src="https://cdn.simpleicons.org/linux/000000/ffffff" width="16"/> Linux · <img src="https://cdn.simpleicons.org/apple/000000/ffffff" width="16"/> macOS (Apple silicon) · <img src="https://cdn.jsdelivr.net/gh/devicons/devicon/icons/windows11/windows11-original.svg" width="16"/> Windows 11 or later |
| **Guest** | <img src="https://cdn.simpleicons.org/linux/000000/ffffff" width="16"/> Linux, always |

### GPU support

The VM gets a GPU through Vulkan on every host, so it can run heavy work like deep learning.

Turn it on with the builder's `gpu` option, and install the guest's half of Vulkan in the image:

```python
import asyncio

from virtx import ConsoleClient, Recipe, ensure_virtx


async def main() -> None:
    await ensure_virtx()
    console = await (
        ConsoleClient.builder()
        .image(
            Recipe("alpine:latest").step(
                # The Vulkan loader, the venus driver that reaches the host's GPU,
                # and vulkaninfo to look at it.
                "apk add --no-cache vulkan-loader mesa-vulkan-virtio vulkan-tools"
            )
        )
        .gpu(True)
        .gpu_memory_mib(8192)
        .build()
    )

    async with console:
        result = await console.exec(["vulkaninfo", "--summary"])

    print(result.stdout.decode(), end="")


asyncio.run(main())
```

The host's GPU shows up in the guest as `Virtio-GPU Venus (<the host's GPU>)`. What the image needs for that:

| | Alpine | Debian |
|---|---|---|
| Vulkan loader (`libvulkan.so.1`) | `vulkan-loader` | `libvulkan1` |
| venus driver | `mesa-vulkan-virtio` | `mesa-vulkan-drivers`, trixie or later |

- **Install the loader yourself.** Without it, no program in the guest finds a Vulkan device, whatever the host has. Some packages bring it along (Debian's `vulkan-tools` does) and some don't (Alpine's doesn't), so name it rather than count on it.
- **An image with no venus driver still runs, but on the CPU.** Debian bookworm's Mesa has no venus, and Vulkan there falls back to `llvmpipe`, a software renderer. Check the `deviceName` your program picks.

If the host can't give a GPU, `build()` fails instead of quietly running on the CPU. On the host, that takes a Vulkan loader too: `libvulkan1` (Debian, Ubuntu) or `vulkan-loader` (Fedora) on Linux, and on Windows `vulkan-1.dll`, which comes with the GPU's driver.

### Filesystem

Mount a host directory into the VM by passing its path.

```python
import asyncio

from virtx import ConsoleClient, Recipe


async def main() -> None:
    console = await (
        ConsoleClient.builder()
        .image(Recipe("alpine:latest"))
        .mount(".", "/project")
        .mount_readonly("/etc", "/host-etc")
        .build()
    )

    async with console:
        result = await console.exec(["sh", "-c", "ls /project && touch /project/hello.txt"])

    print(result.stdout.decode(), end="")


asyncio.run(main())
```

Writes to `/project` land in the host's current directory, while `/host-etc` is read-only: commands in the VM can read it but not write to it.

Going further, you can build a virtual directory in code and mount it into the VM through FUSE.
It mixes files held only in memory with host directories, all under one mount point.

```python
import asyncio
import tempfile

from virtx import ConsoleClient, Directory, HostMount, Recipe


async def main() -> None:
    directory = (
        Directory()
        .with_file("notes/today.md", "ship the release")
        .with_mount("project", ".")
    )
    mount = HostMount(directory, tempfile.mkdtemp())

    console = await (
        ConsoleClient.builder()
        .image(Recipe("alpine:latest"))
        .mount(mount, "/data")
        .build()
    )

    async with console:
        result = await console.exec(["sh", "-c", "cat /data/notes/today.md && ls /data/project"])

    print(result.stdout.decode(), end="")


asyncio.run(main())
```

`notes/today.md` lives only in memory, and `project` is the host's current directory.

Remote stores such as S3, Google Drive and Notion can be mounted the same way, so commands in the VM read them as ordinary files.

```rust
use virtx::{
    console::ConsoleClient,
    fs::{S3Config, S3Fs},
    image::Recipe,
};

#[cfg(target_os = "linux")]
use virtx::fs::FuseMount as HostMount;
#[cfg(target_os = "macos")]
use virtx::fs::FuseTMount as HostMount;
#[cfg(windows)]
use virtx::fs::DokanMount as HostMount;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let bucket = S3Fs::new(&S3Config {
        bucket: "my-bucket".into(),
        region: "us-east-1".into(),
        access_key_id: std::env::var("AWS_ACCESS_KEY_ID")?,
        secret_access_key: std::env::var("AWS_SECRET_ACCESS_KEY")?,
        endpoint: None,
        key_prefix: None,
    })?;

    let mountpoint = std::env::temp_dir().join("virtx-s3");
    std::fs::create_dir_all(&mountpoint)?;
    let mount = HostMount::try_new(bucket, &mountpoint)?;

    let mut console = ConsoleClient::builder()
        .image(Recipe::new("alpine:latest"))
        .mount_readonly(mount, "/s3")
        .build()
        .await?;

    let result = console.exec(["ls", "-R", "/s3"], None).await?;
    print!("{}", String::from_utf8_lossy(&result.stdout));

    Ok(())
}
```

These stores are Rust only for now, each behind its own feature: `s3`, `gdrive` and `notion`.
S3 is read-only, so it's mounted with `mount_readonly`.

This feature needs an extra package installed on macOS and Windows.
The `mount` feature, on by default, mounts a virtx filesystem on the host through the host's FUSE provider:

| Host  | Provider | Needed to build | Needed to mount |
|-------|----------|-----------------|---------------|
| Linux | `/dev/fuse` in the kernel | — | — |
| macOS | [FUSE-T](https://www.fuse-t.org) | — | ✓ |
| Windows | [Dokany](https://github.com/dokan-dev/dokany) | — | ✓ |

A program built with `mount` runs on a host without the provider; only mounting fails, with an error that says what to install.
`virtx::fs::mount_support()` (`mount_support()` in Python, `mountSupport()` in Node) asks ahead of a mount.

On Windows, a Rust program gets that only if it delay-loads `dokan2.dll`, which it has to ask for in its own `build.rs`. virtx's cannot do it on the program's behalf, since a link argument reaches only the package that prints it. The Node and Python packages already do it. Without it, a program that mounts needs Dokany installed just to start.

```rust
// build.rs of a program that depends on virtx
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        println!("cargo::rustc-link-arg=/DELAYLOAD:dokan2.dll");
        println!("cargo::rustc-link-lib=delayimp");
    }
}
```

If you don't mount on the host, build with `default-features = false` and skip all of this.

On macOS:

```sh
brew install --cask fuse-t
```

virtx mounts through FUSE-T 1.x from 1.2.9 on, and is checked against 1.2.9. An older release is refused before a mount: it resets the process's `SIGCHLD` handler on every mount, and unmounting the first of two mounts waits for the second. A FUSE-T of another major version is refused too, with the release to install instead, rather than risk a crash on a layout that changed.

On Windows:

```powershell
winget install --id dokan-dev.Dokany
```

### Images

`.image()` takes any of these:

| Kind | Example | Runs on |
|---|---|---|
| Recipe | `Recipe::new("alpine:latest").step("apk add jq")` | The base, pulled, with the steps built on it for this session only |
| Ref | `ImageSource::reference("jq:latest")` | The build stored under that name, looked up when the VM starts |
| Digest | `ImageSource::digest("sha256:3d75…")` | Exactly that build |

Build an image ahead of time with `ImageClient`, then start any number of VMs on it by its ref or digest:

```rust
use virtx::{
    console::ConsoleClient,
    ensure_virtx,
    image::{ImageClient, ImageSource, Recipe},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    ensure_virtx().await?;
    let mut images = ImageClient::try_new().await?;
    let built = images
        .build(
            Recipe::new("alpine:latest").step("apk add --no-cache jq"),
            Some("jq:latest"), // the ref to store the build under
        )
        .await?;
    println!("{} {}", built.reference, built.digest);

    let mut console = ConsoleClient::builder()
        .image(ImageSource::reference("jq:latest")) // the build stored above
        .build()
        .await?;

    let result = console.exec(["jq", "--version"], None).await?;
    print!("{}", String::from_utf8_lossy(&result.stdout));

    Ok(())
}
```

Rebuilding under the same ref moves it to the new build, while `ImageSource::digest(&built.digest)` stays on this one.  
`images.list()` shows what's built, and `images.remove(...)` deletes an image by its ref or digest.

## How it works

`ConsoleClient` is the part you work with. `build()` boots a Linux VM from the image you named and gives you a console: run commands in it with `exec`, and read and write its files.

The console talks to a small server, `virtx-uvm`, over stdio. Your program starts that server as a child process and it shuts down when you close the console, so there is nothing to install or keep running. `ensure_virtx()` downloads the server into the cache the first time and does nothing after that.

What the VM sees is a filesystem you put together: host folders, files that live only in memory and remote stores, each mounted at a path you choose, read-write or read-only. Network access, CPUs, memory and disk are options on the same builder. The line virtx draws is the one between the VM and your machine; [SECURITY.md](SECURITY.md) says what that covers.

## Requirements

- Python 3.10 or later, Node 18 or later, or Rust.
- Linux, macOS on Apple silicon, or Windows 11 or later as the host.
- Mounting host folders needs FUSE support: nothing extra on Linux, [FUSE-T](https://www.fuse-t.org) on macOS, [Dokany](https://github.com/dokan-dev/dokany) on Windows. See [Filesystem](#filesystem).
- On Windows, the VM runs on the Windows Hypervisor Platform, which is off by default. Turn it on from an administrator PowerShell, then restart with virtualization enabled in the firmware:

```powershell
Enable-WindowsOptionalFeature -Online -FeatureName HypervisorPlatform
```

- GPU access needs a Vulkan loader on the host and the guest's half of Vulkan in the image. See [GPU support](#gpu-support).

## Cache

virtx keeps all persistent state in a single cache directory that you can safely delete at any time:

| Host | Cache directory |
|------|-----------------|
| Linux | `$XDG_CACHE_HOME/virtx`, or `~/.cache/virtx` |
| macOS | `~/Library/Caches/virtx` |
| Windows | `%LOCALAPPDATA%\virtx` |

Set `VIRTX_HOME` to put it somewhere else.

## Contributing

Bug reports, ideas and pull requests are welcome. [CONTRIBUTING.md](CONTRIBUTING.md) covers setup, tests and how to open a PR. For a suspected vulnerability, please follow [SECURITY.md](SECURITY.md) instead of opening a public issue.

Questions and show-and-tell are welcome on [Discord](https://discord.gg/27rx3EJy3P).

## License

Apache-2.0. See [LICENSE](LICENSE).
