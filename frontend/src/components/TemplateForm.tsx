import { signal, effect, computed } from '@preact/signals'
import { useEffect } from 'preact/hooks'
import {
  fetchTemplates,
  fetchTemplatePreview,
  printTemplate,
  TemplateField,
  TemplateList,
} from '../api'

const list = signal<TemplateList | null>(null)
const listError = signal<string | null>(null)
const selectedId = signal<string>('')
const values = signal<Record<string, string>>({})
const fieldErrors = signal<Record<string, string>>({})
const attemptedPrint = signal(false)
const status = signal<{ type: 'success' | 'error'; message: string } | null>(null)
const loading = signal(false)

export const templatePreviewUrl = signal('')

export const templateCustomized = computed(() =>
  Object.values(values.value).some((v) => v.trim() !== '')
)

const selected = computed(
  () => list.value?.templates.find((t) => t.id === selectedId.value) ?? null
)

async function loadTemplates() {
  try {
    const result = await fetchTemplates()
    list.value = result
    listError.value = null
    if (!result.templates.some((t) => t.id === selectedId.value)) {
      selectTemplate(result.templates[0]?.id ?? '')
    }
  } catch (err) {
    listError.value = `${err}`
  }
}

function selectTemplate(id: string) {
  selectedId.value = id
  values.value = {}
  fieldErrors.value = {}
  attemptedPrint.value = false
  status.value = null
}

function setValue(name: string, value: string) {
  values.value = { ...values.value, [name]: value }
}

function setPreview(url: string) {
  const previous = templatePreviewUrl.value
  if (previous.startsWith('blob:')) URL.revokeObjectURL(previous)
  templatePreviewUrl.value = url
}

let previewTimeout: number | null = null

effect(() => {
  const id = selectedId.value
  const currentValues = values.value
  if (previewTimeout) clearTimeout(previewTimeout)
  if (!id) {
    setPreview('')
    return
  }

  previewTimeout = window.setTimeout(async () => {
    try {
      const result = await fetchTemplatePreview(id, currentValues)
      if ('url' in result) {
        setPreview(result.url)
        fieldErrors.value = {}
      } else {
        fieldErrors.value = result.errors ?? {}
        if (!result.errors) status.value = { type: 'error', message: result.error ?? 'Preview failed' }
      }
    } catch (err) {
      console.error('Template preview failed:', err)
    }
  }, 400)
})

/// Show a field's error once it has a value, or after a print attempt (so
/// "required" doesn't shout at an empty form).
function visibleError(field: TemplateField): string | undefined {
  const error = fieldErrors.value[field.name]
  const hasValue = (values.value[field.name] ?? '').trim() !== ''
  return error && (hasValue || attemptedPrint.value) ? error : undefined
}

function FieldInput({ field }: { field: TemplateField }) {
  const id = `template-field-${field.name}`
  const value = values.value[field.name] ?? ''
  const onInput = (e: Event) =>
    setValue(field.name, (e.target as HTMLInputElement | HTMLTextAreaElement).value)
  const error = visibleError(field)
  const common = {
    id,
    name: field.name,
    value,
    required: field.required,
    placeholder: field.placeholder,
    onInput,
  }

  return (
    <div class="form-group">
      <label for={id}>
        {field.label ?? field.name}
        {field.required ? ' *' : ''}
      </label>
      {field.type === 'multiline' ? (
        <textarea {...common} />
      ) : field.type === 'date' ? (
        <input {...common} type="date" />
      ) : field.type === 'url' ? (
        <input {...common} type="url" placeholder={field.placeholder ?? 'https://'} />
      ) : (
        <input {...common} type="text" />
      )}
      {error && <p class="hint error-hint">{error}</p>}
    </div>
  )
}

export function TemplateForm() {
  useEffect(() => {
    loadTemplates()
  }, [])

  const handleSubmit = async (e: Event) => {
    e.preventDefault()
    const template = selected.value
    if (!template) return

    attemptedPrint.value = true
    loading.value = true
    status.value = null
    try {
      const result = await printTemplate(template.id, values.value)
      if (result.success) {
        status.value = { type: 'success', message: result.message || 'Printed!' }
      } else {
        fieldErrors.value = result.errors ?? {}
        status.value = { type: 'error', message: result.error || 'Print failed' }
      }
    } catch (err) {
      status.value = { type: 'error', message: `Error: ${err}` }
    } finally {
      loading.value = false
    }
  }

  const current = list.value
  if (listError.value) return <div class="error">Couldn't load templates: {listError.value}</div>
  if (!current) return <p class="hint">Loading templates…</p>
  if (!current.configured) {
    return (
      <p class="hint">
        No templates directory is configured. Start estrella with{' '}
        <code>--templates-dir</code> (or <code>ESTRELLA_TEMPLATES_DIR</code>) pointing at a folder of
        JSON templates.
      </p>
    )
  }

  const template = selected.value
  return (
    <form onSubmit={handleSubmit}>
      {current.error && <div class="error">{current.error}</div>}
      {current.broken.map((b) => (
        <div class="error" key={b.id}>
          Template <code>{b.id}</code> is broken: {b.error}
        </div>
      ))}
      {status.value && <div class={status.value.type}>{status.value.message}</div>}

      {current.templates.length === 0 ? (
        <p class="hint">No templates found in the templates directory.</p>
      ) : (
        <>
          <div class="form-group">
            <label for="template-select">Template</label>
            <select
              id="template-select"
              value={selectedId.value}
              onChange={(e) => selectTemplate((e.target as HTMLSelectElement).value)}
            >
              {current.templates.map((t) => (
                <option value={t.id} key={t.id}>
                  {t.name}
                </option>
              ))}
            </select>
            {template?.description && <p class="hint">{template.description}</p>}
          </div>

          {template?.fields.map((field) => <FieldInput field={field} key={field.name} />)}

          <button type="submit" disabled={loading.value || !template}>
            {loading.value ? 'Printing...' : 'Print'}
          </button>
        </>
      )}
      {current.skipped.length > 0 && (
        <div class="hint">
          Not loaded:
          <ul>
            {current.skipped.map((s) => (
              <li key={s.name}>
                <code>{s.name}</code>: {s.reason}
              </li>
            ))}
          </ul>
        </div>
      )}
      <button type="button" class="button-secondary" onClick={() => loadTemplates()}>
        Reload templates
      </button>
    </form>
  )
}
