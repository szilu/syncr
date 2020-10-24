SyncR - Fast deduplicating filesystem synchronizer
==================================================

WARNING
-------
THIS IS A WORK IN PROGRESS!
It's also my first attempt to create something in [Rust](https://www.rust-lang.org/) :)

Description
-----------
SyncR is (or will be :) ) an awesome file system synchronization tool for UNIX OS.

Some if it's planned features:
* Content determined chunking with data deduplication (not just in single files, but in the whole directory structure)
* Syncing over SSH connection
* Multi-destination sync: SyncR supports n-way syncing of directories (where in theory n > 100 :) )

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

Current state, TODO
-------------------
* [&#10004;] Directory analyzing
    * [ ] Metadata and chunk cache to speed up scanning
* [&#10004;] Chunking
* Diff algorithms
    * [&#10004;] Latest file
    * [ ] State cache, with interactive conflict resolution
* [&#10004;] Locally available chunk resolution
* [&#10004;] Chunk transfer
* [&#10004;] File write
* [ ] Directory structure creation
* Metadata
    * [&#10004;] File mode (permissions)
    * [ ] Ownership
    * [ ] ...
* [&#10004;] n-way sync support
* [ ] Remote directory support (SSH)

As you can see, my first goal was to make the concept work. And it seems to work pretty well. But some basic things are still missing (for example directory creation is very essential).
