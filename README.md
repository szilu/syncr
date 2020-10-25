SyncR - Fast deduplicating filesystem synchronizer
==================================================

WARNING
-------
THIS IS A WORK IN PROGRESS!
It's also my first attempt to create something in [Rust](https://www.rust-lang.org/).

USE AT YOUR OWN RISK!

I am not responsible, if it destroys your data, eats your breakfast or kills your penguin!

Description
-----------
SyncR is (or will be) an awesome file system synchronization tool for UNIX OS-es.

Some if it's planned features:
* Lightweight, single binary distribution (a statically linked < 1.5MB [MUSL](https://musl.libc.org/) binary can be compiled)
* Content determined chunking with data deduplication (not just in single files, but in the whole directory structure)
* Syncing over SSH connection
* Multi-destination sync: SyncR supports n-way syncing of directories (where in theory n > 100)

The fun thing:
It is possible to run SyncR on a workstation and run n-way synchronization on several servers without a local instance of the synchronized directory.

Installation
------------
    git clone https://github.com/szilu/syncr.git
    cd syncr
    cargo build --release

After compilation copy the binary (target/release/syncr) to your hosts (it must be in PATH to work!).

Basic usage
-----------
    syncr sync ./dir1 ./dir2 [...]

Why
---
I have been using [Unison](https://www.cis.upenn.edu/~bcpierce/unison/) for synchronizing files for years. However, I always hated it's compatibility issues. It's not enough that Unison can't communicate between different versions of itself, but there can be issues with the same version, if they are not compiled with the same [OcaML](https://ocaml.org/) version.

It makes it practically unusable if you want to synchronize between several hosts.

I have looked for other solutions, but I couldn't find a lightweight one, so I decided it will be a good project to learn [Rust](https://www.rust-lang.org/).

The architecture makes it possible to implement other useful filesystem tools on top of it. One example can be a deduplicating backup utility.

Current state, TODO
-------------------
Priorities: H: High, M: Medium, L: Low

* [x] Directory analyzing
    * [ ] Metadata and chunk cache to speed up scanning (M)
* [x] Chunking
* [ ] Diff algorithms
    * [x] Latest file
    * [ ] State cache, with interactive conflict resolution (H)
* [x] Locally available chunk resolution
* [x] Chunk transfer
* [x] File write
* [ ] Directory structure creation (H)
* [ ] Metadata
    * [x] File mode (permissions)
    * [ ] Ownership (L)
    * [ ] ...
* [x] n-way sync support
* [x] Remote directory support (SSH)
* [ ] Error handling (H)
* [ ] Configuration
    * [ ] Include / exclude lists (H)
    * [ ] Selecting diff algorithm (H)
    * [ ] Run in batch mode (M)
    * [ ] Verbose / Silent / Progress / Debug (L)
    * [ ] Metadata masks / overrides (permissions, user/group) (L)
    * [ ] Store archive metadata on master host / master + all hosts (L)
    * [ ] Remote shell command (L)

As you can see, my first goal was to make the concept work. And it seems to work pretty well. But some basic things are still missing (for example directory creation is very essential).
