# httpfs-rs

An alternative to [httpfs2](https://httpfs.sourceforge.net/).

```
Usage: httpfs-rs <mountpoint> [--url <url>] [-f <file>] [-p <proxy>]

Mount HTTP resources as a file system using FUSE

Positional Arguments:
  mountpoint        the mount point (directory) to use.

Options:
  --url             URL to download (mutually exclusive with 'file')
  -f, --file        the file (list of URLs) to process (mutually exclusive with 'url')

  -p, --proxy       proxy to use
  --help            display usage information
```