# Scribble: simple instructional videos

Scribble is a program for creating simple instructional videos. There are plenty of fancy programs
out there for making beautiful animations -- this is not one of them. It only aims to be an
improvement (both in terms of output quality and creation effort) over the venerable 
doc-cam + microphone method for creating video lectures.

# This is ALPHA software.

It is likely to crash and eat your hard work. Even if it doesn't the file format is subject to
change, and so future versions of Scribble won't be able to open current save files.
 *Do not* use this for anything important!

 # How to run

 Scribble is written in the [`rust`](www.rust-lang.org) programming language. In order to install
 it, you'll need to first [install a `rust` compiler](https://www.rust-lang.org/tools/install).
 You might need install some [`gstreamer`] plugins (at least `vp9enc` and `webmmux`),
 because Scribble uses gstreamer for encoding videos. (If you're on linux, it should be enough
 to install a package with a name similar to `gstreamer1.0-plugins-good`.)

 Once your rust compiler and gstreamer plugins are ready, you should be able to run Scribble
 by cloning this git repository, opening it in a terminal, and typing `cargo run --release`.