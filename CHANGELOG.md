# Changelog

## Unreleased

- Terminal texture updates now land in the same frame they're drawn
  (previously lagged one frame behind `Tui::draw()`).
- Redrawing only some rows of a terminal (e.g. a blinking cursor or a
  single changed line) now costs proportionally to the rows that changed,
  instead of re-rendering the whole grid every dirty frame.
