(tag_name) @tag

(erroneous_end_tag_name) @tag

(doctype) @tag.doctype

(attribute_name) @attribute

[
  "\""
  "'"
  (attribute_value)
] @string

(comment) @comment

(entity) @string.special

"=" @punctuation.delimiter

[
  "<"
  ">"
  "<!"
  "</"
  "/>"
] @punctuation.bracket
