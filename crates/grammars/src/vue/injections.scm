; <script> without a lang attribute
((script_element
  (start_tag) @_no_lang
  (raw_text) @injection.content)
  (#not-match? @_no_lang "lang=")
  (#set! injection.language "javascript"))

; <script lang="js">
((script_element
  (start_tag
    (attribute
      (attribute_name) @_lang
      (quoted_attribute_value
        (attribute_value) @_js)))
  (raw_text) @injection.content)
  (#eq? @_lang "lang")
  (#eq? @_js "js")
  (#set! injection.language "javascript"))

; <script lang="ts">
((script_element
  (start_tag
    (attribute
      (attribute_name) @_lang
      (quoted_attribute_value
        (attribute_value) @_ts)))
  (raw_text) @injection.content)
  (#eq? @_lang "lang")
  (#eq? @_ts "ts")
  (#set! injection.language "typescript"))

; <script lang="tsx"> / <script lang="jsx">
((script_element
  (start_tag
    (attribute
      (attribute_name) @_lang
      (quoted_attribute_value
        (attribute_value) @_tsx)))
  (raw_text) @injection.content)
  (#eq? @_lang "lang")
  (#any-of? @_tsx "tsx" "jsx")
  (#set! injection.language "tsx"))

; {{ expression }}
((interpolation
  (raw_text) @injection.content)
  (#set! injection.language "typescript"))

; v-if="expression", :prop="expression", @event="handler"
(directive_attribute
  (quoted_attribute_value
    (attribute_value) @injection.content)
  (#set! injection.language "typescript"))

; <style lang="scss"> etc.
(style_element
  (start_tag
    (attribute
      (attribute_name) @_attr_name
      (#eq? @_attr_name "lang")
      (quoted_attribute_value
        (attribute_value) @injection.language)))
  (raw_text) @injection.content)

; <style> / <style scoped> without a lang attribute
((style_element
  (start_tag) @_no_lang
  (raw_text) @injection.content)
  (#not-match? @_no_lang "lang=")
  (#set! injection.language "css"))

; <template lang="pug">
((template_element
  (start_tag
    (attribute
      (attribute_name) @_lang
      (quoted_attribute_value
        (attribute_value) @_pug)))
  (text) @injection.content)
  (#eq? @_lang "lang")
  (#eq? @_pug "pug")
  (#set! injection.language "pug"))

; <!-- comments --> are HTML
((comment) @injection.content
  (#set! injection.language "html"))
