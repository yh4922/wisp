; Based on the queries shipped with tree-sitter-nginx (opa-oz).
(comment) @comment

(value) @variable

(attribute
  (keyword) @property)

[
  (location_modifier)
  "="
] @operator

[
  "location"
  "if"
  "map"
] @keyword

(directive
  (keyword) @keyword)

(boolean) @boolean

[
  (auto)
  (constant)
  (level)
  (connection_method)
] @constant.builtin

(var) @variable.special

(if
  condition: (condition) @embedded)

[
  (string_literal)
  (quoted_string_literal)
  (file)
  (mask)
] @string

(directive
  (variable) @variable.parameter)

(location_route) @string.special

";" @punctuation.delimiter

[
  (numeric_literal)
  (time)
  (size)
  (cpumask)
] @number

[
  "{"
  "}"
] @punctuation.bracket
