(comment) @comment

(doctype) @tag.doctype

(start_tag) @tag

(end_tag) @tag

(self_closing_tag) @tag

((tag_name) @tag
  (#match? @tag "^[a-z]"))

((tag_name) @type
  (#match? @type "^[A-Z]"))

(erroneous_end_tag_name) @tag

(attribute_name) @attribute

(directive_attribute) @property

(quoted_attribute_value) @string

(attribute_value) @string

(entity) @string.special

(interpolation
  [
    "{{"
    "}}"
  ] @punctuation.special)

((directive_name) @punctuation.delimiter
  (#any-of? @punctuation.delimiter ":" "@" "#"))

((directive_name) @keyword
  (#match? @keyword "^v-"))

(directive_value) @attribute

(dynamic_directive_value) @attribute

(directive_modifier) @attribute

[
  "="
  ":"
  "."
] @punctuation.delimiter

[
  "<"
  ">"
  "</"
  "/>"
  "<!"
  "["
  "]"
] @punctuation.bracket
