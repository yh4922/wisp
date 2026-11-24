(comment) @annotation

(attribute
  (keyword) @name
  (block)) @item

(location
  "location" @context
  (location_modifier)? @context
  (location_route) @name) @item

(if
  "if" @context
  condition: (condition) @name) @item

(map
  "map" @context) @item
