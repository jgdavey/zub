# zub: organize zubprogramz

Zub is a model for setting up shell programs that use subcommands, like `git` or `rbenv`. Building a zub program does not require you to write shell scripts in bash — you can write subcommands in any scripting language you prefer.

Zub started as a fork of `qrush/sub`, but has diverged enough that a new name and repo felt warranted.

A zub program is run at the command line using this style:

    $ [name of program] [subcommand] [(args)]

Here's some quick examples:

    $ rbenv                    # prints out usage and subcommands
    $ rbenv versions           # runs the "versions" subcommand
    $ rbenv shell 1.9.3-p194   # runs the "shell" subcommand, passing "1.9.3-p194" as an argument

Each subcommand maps to a separate, standalone executable program. Zub programs are laid out like so:

    .
    ├── zub.yml           # declares your program's name (and optional metadata)
    ├── bin/<name>        # entrypoint shim that hands off to the shared zub binary
    ├── completions       # (optional) bash/zsh completions
    ├── libexec           # where the subcommand executables are
    └── share             # static data storage

The `zub.yml` at the root names your program. A single shared `zub` binary
provides the dispatcher and every built-in command; it learns *which* program it
is from the config file you point it at, so there's no build step or
source-templating involved in making a program:

``` yaml
name: rush
# optional:
version: 0.1.0
description: A delicious way to organize programs
```

You run your program through `bin/<name>`, a tiny generated shim that re-invokes
the shared `zub` binary with your config:

``` sh
#!/bin/sh
here="$(cd "$(dirname "$0")/.." && pwd)"
exec zub -C "$here/zub.yml" "$@"
```

The `zub` binary itself must be on your `PATH`. The shim derives your program's
root from the location of `zub.yml` (its parent directory), so you can move the
program tree anywhere and it keeps working. You can also invoke
`zub -C /path/to/zub.yml <subcommand>` directly, or export `ZUB_CONFIG=/path/to/zub.yml`
so a bare `zub <subcommand>` knows which program to run. (An explicit `-C` always
wins over `ZUB_CONFIG`.)

## Subcommands

Each subcommand executable does not necessarily need to be in bash. It can be any program, shell script, or even a symlink. It just needs to run.

Here's an example of adding a new subcommand. Let's say your program is named `rush`. Run:

    touch libexec/rush-who
    chmod a+x libexec/rush-who

Now open up your editor, and dump in:

``` bash
#!/usr/bin/env bash
set -e

who
```

Of course, this is a simple example... but now `rush who` should work!

    $ rush who
    qrush     console  Sep 14 17:15 

You can run *any* executable in the `libexec` directly, as long as it follows the `NAME-SUBCOMMAND` convention. Try out a Ruby script or your favorite language!

## What's built in

You get a few commands that come with every zub program:

* `commands`: Prints out every subcommand available.
* `completions`: Helps kick off subcommand autocompletion.
* `help`: Document how to use each subcommand.
* `init`: Shows how to load your program with autocompletions, based on your shell.
* `new`: Generates a new subcommand script (pre-filled with front-matter).
* `source`: Prints the source of a subcommand.

These are built into the `zub` binary. If you ever need to replace one with your
own script, name a `libexec` command after it and add `override: true` to that
script's front-matter (see below) — otherwise the built-in always wins.

(Creating a brand new program is handled by the separate `zub-scaffold` tool, not
a built-in — see "Make your own program" below.)

If you ever need to reference files inside of your program's installation, say to access a file in the `share` directory, your program exposes the directory path in the environment, based on its name. For a program named `rush`, the variable name will be `_RUSH_ROOT`.

Here's an example subcommand you could drop into your `libexec` directory to show this in action: (make sure to correct the name!)

``` bash
#!/usr/bin/env bash
set -e

echo $_RUSH_ROOT
```

You can also use this environment variable to call other commands inside of your `libexec` directly. Composition of this type very much encourages reuse of small scripts, and keeps scripts doing *one* thing simply.

## Self-documenting subcommands

Each subcommand can opt into self-documentation, which allows the subcommand to provide information when `rush` and `rush help [SUBCOMMAND]` is run.

This is done with a small block of *front-matter* at the top of the script: a
run of contiguous lines that begin with your comment character followed by `@`
(`#@` for shell/Ruby/Python, `//@` for JavaScript, `;@` for Lisp, `--@` for
SQL/Lua). The text after the sigil is plain [YAML](https://yaml.org/). The
parser reads the block, stops at the first non-sigil line, and never touches the
rest of your script — so it stays fast no matter how big your command is.

Here's an example from `rush who`:

``` bash
#!/usr/bin/env bash
#@ usage: rush who
#@ summary: Check who's logged in
#@ help: |
#@   This will print out when you run `rush help who`.
#@   You can have multiple lines even!
#@
#@      Show off an example indented
#@
#@   And maybe start off another one?

set -e

who
```

The recognized keys are `summary`, `usage`, `help`, `complete`, and `override`
(unknown keys are ignored, so the format can grow without breaking older
scripts). Because the body is real YAML, multi-line help uses a `|` block scalar
and indentation is preserved.

Now, when you run `rush`, the "summary" will show up:

    usage: rush <command> [<args>]

    Some useful rush commands are:
       commands               List all rush commands
       who                    Check who's logged in

And running `rush help who` will show the "usage" line, and then the "help" block:

    Usage: rush who

    This will print out when you run `rush help who`.
    You can have multiple lines even!

       Show off an example indented

    And maybe start off another one?

That's not all you get by convention with zub...

## Autocompletion

Your program loves autocompletion. It's the mustard, mayo, or whatever topping you'd like that day for your commands. Just like real toppings, you have to opt into them! Zub provides two kinds of autocompletion:

1. Automatic autocompletion to find subcommands (What can this program do?)
2. Opt-in autocompletion of potential arguments for your subcommands (What can this subcommand do?)

Opting into argument autocompletion takes two things: declare `complete: true`
in your script's front-matter, and have the script handle a `--complete` flag.
Here's an example modeled on rbenv's `whence`:

``` bash
#!/usr/bin/env bash
#@ summary: List something completable
#@ complete: true
set -e

if [ "$1" = "--complete" ]; then
  echo --path
  exec rbenv shims --short
fi

# lots more bash...
```

The `complete: true` key lets the core know the script participates in
completion without scanning its body — commands that don't declare it simply
fall back to your shell's default (filename) completion.

Passing the `--complete` flag to this subcommand short circuits the real command, and then runs another subcommand instead. The output from your subcommand's `--complete` run is sent to your shell's autocompletion handler for you, and you don't ever have to once worry about how any of that works!

Run the `init` subcommand after you've prepared your program to get it loading automatically in your shell.

## Shortcuts

Creating shortcuts for commands is easy, just symlink the shorter version you'd like to run inside of your `libexec` directory.

Let's say we want to shorten up our `rush who` to `rush w`. Just make a symlink!

    cd libexec
    ln -s rush-who rush-w

Now, `rush w` should run `libexec/rush-who`, and save you mere milliseconds of typing every day!

## Make your own program

Use the `zub-scaffold` tool to generate a fresh program tree:

    zub-scaffold rush

This creates a `rush/` directory containing a `zub.yml` (with `name: rush`), a
self-locating `bin/rush` shim, the completion scripts, an example `libexec/who`
command, and an empty `share` directory. There's no source-templating or build
step — your program's identity comes entirely from `zub.yml`. (Give it a better
name than `rush`!)

By default `zub-scaffold` refuses to touch an existing directory. To refresh the
generated files of a program you already have — after upgrading `zub`, say — run
it with `--regenerate` from the program's **parent** directory:

    zub-scaffold rush --regenerate            # ask before replacing each existing file
    zub-scaffold rush --regenerate=clobber    # replace them all, no prompts

Regeneration only ever rewrites the files `zub-scaffold` generates (`zub.yml`,
`bin/rush`, the completion scripts, and the example `libexec/who`); missing ones
are written silently. Your own `libexec` commands and `share` contents are never
touched.

## Install zub and your program

First, make the shared `zub` binary (and `zub-scaffold`) available on your
`PATH`. The easiest way is [`mise`](https://mise.jdx.dev/), which pulls a
prebuilt binary from the GitHub releases:

    mise use -g 'ubi:jgdavey/sub[exe=zub]'

Or grab a release tarball directly (one per platform — `aarch64-apple-darwin`,
`x86_64-apple-darwin`, `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`)
and drop both binaries on your `PATH`:

    curl -L https://github.com/jgdavey/sub/releases/download/v0.1.0/zub-0.1.0-<target>.tar.gz \
      | tar xz && mv zub zub-scaffold ~/.local/bin/

Or build from this repo:

    cargo install --path .

…which puts `zub` and `zub-scaffold` in `~/.cargo/bin`.

Then load your program in your shell. Say your program lives at `$HOME/.rush`:

For bash users:

    echo 'eval "$($HOME/.rush/bin/rush init - bash)"' >> ~/.bash_profile
    exec bash

For zsh users:

    echo 'eval "$($HOME/.rush/bin/rush init - zsh)"' >> ~/.zshenv
    source ~/.zshenv

`init` derives the `PATH` entries and completion wiring from your `zub.yml`, and
defines a shell function that runs `zub -C <your config>` under the hood.

## Roadmap

- [x] Provide a script to convert script headers from old to new format
- [x] Bring completion scripts over during scaffold
- [x] Add an example script (bash?) during scaffold
- [x] Handle namespaced commands
- [x] scaffold: overwrite existing
- [x] Set up CI building
- [ ] Cache indexed commands (if perf becomes an issue)
- [ ] Document how to do completion
- [ ] static completion in front-matter?
- [ ] Add dynamic help support (calling `<name> <sub> --help` instead of front-matter)

## License

MIT. See `LICENSE`.
