; Template text outside of <?php ... ?> is HTML.
((text) @injection.content
  (#set! injection.language "html")
  (#set! injection.combined))

; <<<SQL ... SQL heredocs use their delimiter as the language name.
((heredoc_body)
  (heredoc_end) @injection.language) @injection.content

((nowdoc_body)
  (heredoc_end) @injection.language) @injection.content
