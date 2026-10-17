# Tool evidence checkpoint (ticket06)

This is implementation evidence, not approval for OS-managed cleanup scopes. No live cleanup ran.

Docker's verbose formatter emits an object with Images, Containers, Volumes and BuildCache. Outside table mode, image IDs remain untruncated. Image `Containers`, `UniqueSize` and `SharedSize` are strings; sizes are rounded human-readable text. The parser keeps those strings and never sums image virtual sizes or calls them host reclaimed bytes. Sources: [Docker CLI verbose formatter, v28.5.1](https://github.com/docker/cli/blob/v28.5.1/cli/command/formatter/disk_usage.go), [image formatter](https://github.com/docker/cli/blob/master/cli/command/formatter/image.go).

Native daemon queries pin an explicitly validated local Unix socket or Windows named pipe and remove inherited endpoint/context override variables. The selected context is read before and after; remote/TCP/SSH endpoints and changing contexts are refused. This still needs installed-version and before/after execution proof before enabling cleanup. [Docker context behavior](https://docs.docker.com/engine/manage-resources/contexts/).

For image removal the candidate design uses approved full content-addressed IDs, no force, and `--no-prune` to avoid removing unapproved parents; a newly unused image never joins an existing selection. Build-cache selectors need their own builder identity/capability proof and are not enabled by the image parser. [Docker image rm](https://docs.docker.com/reference/cli/docker/image/rm/), [Buildx prune](https://docs.docker.com/reference/cli/docker/buildx/prune/).

Simulator device JSON can identify unavailable devices by exact UUID. That does not establish that an old runtime is unused by recent projects; no runtime removal is enabled. Installed macOS command/schema/installation-identity proof is outstanding. [Apple DTS discussion](https://developer.apple.com/forums/thread/835883).

Apple's Jan 28 2026 guidance says Logic Pro12/MainStage4 consolidate the library as `Logic Pro Library.bundle` in Music, and clone it on APFS without extra storage. Legacy copies therefore do not imply reclaimable bytes. Current navigation is Logic Pro > Sound Library > Manage Packs. Advice must remain version-aware and never instruct Vacancy to delete these files. [Apple sound library guidance](https://support.apple.com/en-us/125036).

The Windows supported operations and elevated-helper boundary remain governed by the pending semantic-scope decision recorded in the run's preflight. Existing filesystem Guard denial of system/cloud/app paths remains intact.
