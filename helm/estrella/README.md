# estrella Helm chart

Runs the estrella web UI and JSON API, printing to a network printer over raw
TCP (port 9100). Bluetooth printers aren't supported in Kubernetes.

## Install

The chart is published as an OCI artifact to GitHub Packages:

```bash
helm install estrella oci://ghcr.io/blybot/helm/estrella --version <version> \
  --set printer.protocol=escpos \
  --set printer.address=192.168.1.50 \
  --set ingress.host=estrella.example.com
```

The chart's `appVersion` matches the image tag (`ghcr.io/blybot/estrella:<version>`).
New GitHub packages are private by default: make the package public, or add an
`imagePullSecrets` entry.

> [!WARNING]
> The API has no authentication. Anyone who can reach it can print, make the
> server fetch image URLs, and upload photos. Restrict access at the ingress;
> see the example below.

## Values

| Value | Default | Description |
|-------|---------|-------------|
| `printer.protocol` | **required** | `escpos` (Xprinter, Epson TM, ...) or `star` (StarPRNT) |
| `printer.address` | **required** | Printer `host` or `host:port`; the port defaults to 9100 |
| `image.repository` | `ghcr.io/blybot/estrella` | |
| `image.tag` | chart `appVersion` | `latest` is rejected |
| `image.pullPolicy` | `IfNotPresent` | |
| `imagePullSecrets` | `[]` | |
| `templates.existingConfigMap` | `""` | ConfigMap of `*.json` templates for the web UI's Templates tab, mounted read-only |
| `service.port` | `80` | |
| `ingress.enabled` | `true` | |
| `ingress.className` | `""` | e.g. `nginx` |
| `ingress.host` | **required** when the ingress is enabled | |
| `ingress.annotations` | `{}` | Auth, body size, cert-manager, ... |
| `ingress.tls.enabled` | `false` | |
| `ingress.tls.secretName` | `<release>-tls` | |
| `resources` | 50m CPU, 256Mi memory | |
| `extraEnv` | `[]` | Extra container env vars; rendered last, so they can override the chart's |
| `nodeSelector`, `tolerations`, `affinity` | empty | |

Unknown values are rejected by `values.schema.json`.

## ingress-nginx example

Photo uploads can be up to 50 MB, above ingress-nginx's default 1 MB body
limit. This also adds basic auth:

```bash
htpasswd -c auth admin
kubectl create secret generic estrella-basic-auth --from-file=auth
```

```yaml
printer:
  protocol: escpos
  address: 192.168.1.50
ingress:
  className: nginx
  host: estrella.example.com
  annotations:
    nginx.ingress.kubernetes.io/proxy-body-size: 50m
    nginx.ingress.kubernetes.io/auth-type: basic
    nginx.ingress.kubernetes.io/auth-secret: estrella-basic-auth
  tls:
    enabled: true
```

## Templates

The Templates tab fills JSON document templates from a form (see the main
README for the format). Keep your templates in a ConfigMap and point the
chart at it:

```bash
kubectl create configmap estrella-templates --from-file=templates/
helm upgrade estrella oci://ghcr.io/blybot/helm/estrella --reuse-values \
  --set templates.existingConfigMap=estrella-templates
```

Templates are re-read on every request, so after updating the ConfigMap
they show up once the kubelet syncs it (up to about a minute), with no
restart. Broken templates are listed in the tab with the reason.

## Notes

- One replica with `strategy: Recreate`: there's one printer, and uploaded
  photos are held in memory between the upload and print requests.
- `/healthz` (readiness and liveness) doesn't contact the printer, so a printer
  that's switched off doesn't take the web UI down. Print requests fail with an
  error instead.
- There are no metrics: estrella doesn't expose any yet.
- Test the chart with `scripts/test-helm-chart.sh`.
