# Reovim

_add nvim like capabilities to terminal app's_

Reovim provides a simple way to add an extensible low resource usage editing component to terminal app's.

## Motivation
When creating terminal app's it's common to open up files edit them in an editor then pipe that result into some cli in the unix philosophy.
This is great and works perfectly! But as lazy developer sometimes I want my tui app's do be able to do complicated editing behaviour embedded in the application,
for example _sockman_ aims to support snippets and other nice to haves directly inside a single executable for quickly getting to grips with an websocket's applications features.

### Features
- :rocket: *fast* maintains near real time feedback.
- :wrench: *extensible* plugin's provide specific functionality.
- :technologist: *low profile* memory, thread's, and cpu treated as a finite resource.
- :pencil2: *edit* the world as buffers, maintains growable buffer's for representing text.
- :package: *portable* ship as a single binary, use in a _lot of places_.

## Road Map Features
- [x] open a file
- [x] display text
- [ ] edit like vim https://vim.rtorr.com/
- [ ] save file


### Stretch Goals
- [ ] plugin architecture


### Plugin Architecture

Inspired by bevy's plugin architecture reovim exposes a trait that when implemented allow's for configuration of the editor.
This will be the main configuration strategy, by using plugin's you can decide how reovim fit's into your tui application.


