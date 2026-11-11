{{- define "estrella.selectorLabels" -}}
app.kubernetes.io/name: estrella
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{- define "estrella.labels" -}}
{{ include "estrella.selectorLabels" . }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version }}
{{- end }}
