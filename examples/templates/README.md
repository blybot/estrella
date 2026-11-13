# Example templates

Templates for the web UI's Templates tab. Start the server with
`estrella serve --templates-dir examples/templates` to try them.

Only `*.json` files are templates: other files (like this one) are listed in
the Templates tab as "not loaded". When mounting templates from a Kubernetes
ConfigMap, the keys must end in `.json` too.

See "Templates" in the main README for the format.
