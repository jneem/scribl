# Scribble: simple instructional videos
![Build](https://github.com/jneem/scribble/workflows/Rust/badge.svg)

Scribble is a program for creating simple instructional videos. There are plenty of fancy programs
out there for making beautiful animations -- this is not one of them. It only aims to be an
improvement (both in terms of output quality and creation effort) over the venerable 
doc-cam + microphone method for creating video lectures.

# A sample

Here's a short video created with scribble:

[![Sample video](https://img.youtube.com/vi/MB7anfjTe9I/hqdefault.jpg)](https://youtu.be/MB7anfjTe9I)

Here's a screenshot of its user interface:

![Screenshot](https://raw.githubusercontent.com/jneem/scribble/master/scribble/sample/screenshot.png)

# This is ALPHA software.

It is likely to crash and eat your hard work. Even if it doesn't the file format is subject to
change, and so future versions of Scribble won't be able to open current save files.
*Do not* use this for anything important!

# How to run

Scribble is written in the [`rust`](www.rust-lang.org) programming language. In order to install
it, you'll need to first [install a `rust` compiler](https://www.rust-lang.org/tools/install).
Then you'll need to install some dependencies (because although rust manages rust-written dependencies
very easily, scribble also depends on some software written in C). If you're running linux,
you'll need to install (if you don't have them already) development packages for

- GTK+-3
- pango
- gstreamer
- alsa
- atk

You might need install some [`gstreamer`](gstreamer.freedesktop.org) plugins (at least `vp9enc` and `webmmux`),
because Scribble uses gstreamer for encoding videos. (If you're on linux, it should be enough
to install a package with a name similar to `gstreamer1.0-plugins-good`.)

Once your rust compiler and gstreamer plugins are ready, you should be able to run Scribble
by cloning this git repository, opening it in a terminal, and typing `cargo run --release`.