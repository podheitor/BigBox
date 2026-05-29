// Vorcaro's Studio — i18n runtime + dictionary (PT-BR / EN).
// Loaded BEFORE studio.js so `t()` and `applyStaticI18n()` are globals.
//
// Usage:
//   t('tab.contacts')                  → "Contatos" / "Contacts"
//   t('toast.error', { err })          → interpolates {err}
//   applyStaticI18n()                  → fills every [data-i18n*] element
//   setLang('en')                      → persists + re-applies + re-renders
//
// Interpolation only runs when a `vars` object is passed, so dictionary
// values that legitimately contain braces (e.g. the {firstname} variable
// hints) survive untouched when looked up without vars.

const FALLBACK_LANG = 'pt-BR';
const SUPPORTED_LANGS = ['pt-BR', 'en'];
const LANG_STORAGE_KEY = 'vorcaro.lang';

const I18N = {
  'pt-BR': {
    // ── Header / tabs ──────────────────────────────────────────
    'app.subtitle': 'mensagens em lote para WhatsApp & Telegram',
    'tab.contacts': 'Contatos',
    'tab.lists': 'Listas',
    'tab.campaign': 'Campanha',
    'tab.logs': 'Logs',
    'tab.settings': 'Configurações',

    // ── Contacts toolbar ───────────────────────────────────────
    'contacts.search.placeholder': 'Buscar por nome, telefone ou tag…',
    'contacts.tagFilter.all': 'Todas as tags',
    'scrape.workspace.title': 'Conta a raspar',
    'scrape.label.title': 'Filtrar por etiqueta antes de raspar',
    'scrape.label.none': '— sem filtro de etiqueta —',
    'btn.refreshLabels.title': 'Carregar etiquetas desta conta',
    'btn.debugDom.title': 'Diagnosticar DOM do topo do painel (cole o resultado pro dev)',
    'btn.scrape': 'Raspar',
    'btn.scrape.title': 'Raspar conversas da conta selecionada',
    'btn.newContact': '+ Novo contato',
    'btn.importCsv': 'Importar CSV',

    // ── Bulk bar ───────────────────────────────────────────────
    'bulk.count': '{n} selecionado(s)',
    'btn.bulkTag': 'Aplicar tag…',
    'btn.bulkUntag': 'Remover tag…',
    'btn.bulkAddList': 'Adicionar à lista…',
    'btn.bulkDelete': 'Apagar',

    // ── Contacts table ─────────────────────────────────────────
    'contacts.col.name': 'Nome',
    'contacts.col.whatsapp': 'WhatsApp',
    'contacts.col.waBusiness': 'WA Business',
    'contacts.col.telegram': 'Telegram',
    'contacts.col.tags': 'Tags',
    'contacts.empty': 'Sem contatos ainda. Adicione manualmente, importe um CSV, ou raspe a conta selecionada acima (a conta precisa estar aberta e logada no BigBox).',
    'contacts.tagsHint': '<strong>Tags:</strong> rótulos que você cria. Edite um contato e preencha a caixa <em>Tags</em> (separe por vírgula), ou selecione vários contatos e use <em>Aplicar tag…</em> na barra acima. Filtre por tag no menu suspenso ao lado da busca, ou use “Por tag” na aba <strong>Campanha</strong>.',
    'btn.edit': 'Editar',
    'btn.delete': 'Apagar',

    // ── Lists ──────────────────────────────────────────────────
    'btn.newList': '+ Nova lista',
    'lists.empty': 'Nenhuma lista ainda. Listas agrupam contatos para envio em campanhas.',
    'list.name.placeholder': 'Nome da lista',
    'btn.renameList': 'Renomear',
    'btn.deleteList': 'Apagar lista',
    'list.members.heading': 'Membros',
    'list.add.heading': 'Adicionar contatos',
    'list.add.search.placeholder': 'Buscar contatos para adicionar…',
    'list.member.remove': 'Remover',
    'list.add.add': 'Adicionar',
    'list.empty.members': 'Lista vazia',
    'list.add.noCandidates': 'Sem candidatos',
    'list.meta.count': '{n} contato(s)',

    // ── Campaign ───────────────────────────────────────────────
    'campaign.heading': 'Nova campanha',
    'campaign.name.label': 'Nome da campanha',
    'campaign.name.placeholder': 'Ex.: Promo de aniversário',
    'campaign.platform.label': 'Plataforma',
    'platform.whatsapp_web': 'WhatsApp (Web)',
    'platform.whatsapp_business_web': 'WhatsApp Business (Web)',
    'platform.telegram': 'Telegram',
    'platform.whatsapp_cloud_api': 'WhatsApp Cloud API (Meta)',
    'campaign.workspace.label': 'Conta (workspace) — qual aba do BigBox vai mandar',
    'campaign.target.label': 'Destinatários',
    'campaign.target.list': 'Por lista salva',
    'campaign.target.tag': 'Por tag',
    'campaign.target.adhoc': 'Contatos selecionados na aba Contatos',
    'campaign.list.label': 'Lista',
    'campaign.list.none': '— sem listas —',
    'campaign.tag.label': 'Tag',
    'campaign.tag.none': '— sem tags —',
    'campaign.adhoc.pre': 'Use a aba <strong>Contatos</strong> para selecionar os destinatários via as checkboxes.',
    'campaign.adhoc.post': 'selecionado(s).',
    'campaign.cloud.msgType.label': 'Tipo de mensagem (Cloud API)',
    'campaign.cloud.msgType.template': 'Template aprovado (recomendado)',
    'campaign.cloud.msgType.freeform': 'Texto livre (só funciona dentro da janela 24h)',
    'campaign.template.label': 'Template',
    'campaign.body.label': 'Mensagem — variáveis: <code>{nome}</code>, <code>{firstname}</code>, <code>{whatsapp}</code>, <code>{telegram}</code>, <code>{tag}</code>, <code>{notes}</code>',
    'campaign.body.placeholder': 'Olá {firstname}, …',
    'btn.emoji': '😀 Emojis',
    'btn.emoji.title': 'Inserir emoji',
    'campaign.emoji.hint': 'ou use o atalho do sistema (KDE: <kbd>Meta+.</kbd>, GNOME: <kbd>Ctrl+;</kbd>)',
    'campaign.attach.label': 'Anexo (opcional — imagem, vídeo, documento ou áudio)',
    'btn.pickAttachment': '+ Anexar arquivo',
    'campaign.schedule.label': 'Agendar envio (opcional — deixe em branco para iniciar agora)',
    'btn.preview': 'Pré-visualizar',
    'btn.start': 'Iniciar envio',
    'btn.start.schedule': 'Agendar envio',
    'campaign.recent.heading': 'Campanhas recentes',
    'template.noParams': 'Template sem parâmetros.',
    'template.paramsHint': 'Parâmetros do template — use variáveis tipo <code>{firstname}</code> ou texto fixo:',
    'template.param': 'Param {i}',
    'template.none': '— nenhum template APROVADO encontrado —',
    'template.loadError': '— erro ao carregar —',
    'attach.remove': 'remover',
    'campaign.unnamed': 'Campanha sem nome',
    'campaign.recent.none': '— nenhuma campanha ainda —',
    'campaign.recent.meta': '{platform} · {n} envio(s)',

    // ── Campaign preview ───────────────────────────────────────
    'preview.recipients': 'Destinatários',
    'preview.withHandle': 'Com identificador para {platform}',
    'preview.missingHandle': 'Sem identificador (serão pulados)',
    'preview.dailyRemaining': 'Saldo diário restante',
    'preview.warn': '⚠ Mais de {threshold} destinatários. Envios em massa pelo WhatsApp Web podem causar <strong>banimento da conta</strong>. Considere usar listas menores ou aumentar o atraso em Configurações.',

    // ── Logs ───────────────────────────────────────────────────
    'logs.pause': 'Pausar',
    'logs.resume': 'Retomar',
    'logs.abort': 'Abortar',
    'logs.col.when': 'Quando',
    'logs.col.contact': 'Contato',
    'logs.col.status': 'Status',
    'logs.col.error': 'Erro',
    'logs.empty': 'Selecione uma campanha acima para ver o progresso.',
    'logs.option.select': '— selecione uma campanha —',
    'logs.summary.total': 'Total:',
    'logs.summary.sent': '✓ Enviados:',
    'logs.summary.failed': '✗ Falha:',
    'logs.summary.invalid': '# Número inválido:',
    'logs.summary.skipped': '↷ Pulados:',
    'logs.noSends': 'Sem envios ainda',

    // ── Settings ───────────────────────────────────────────────
    'settings.heading': 'Configurações de envio',
    'settings.hint': 'Os <em>defaults</em> são conservadores para reduzir o risco de banimento do WhatsApp.',
    'settings.minDelay': 'Atraso mínimo entre envios (segundos)',
    'settings.maxDelay': 'Atraso máximo entre envios (segundos)',
    'settings.dailyCap': 'Limite diário por plataforma',
    'settings.warn': 'Aviso quando destinatários >',
    'settings.autopause': 'Pausa automática após N falhas seguidas',
    'settings.retries': 'Tentativas extras por destinatário em caso de falha',
    'btn.saveSettings': 'Salvar',
    'settings.saved': 'Salvo ✓',
    'settings.language.label': 'Idioma / Language',
    'cloud.heading': 'WhatsApp Cloud API (Meta)',
    'cloud.hint': 'Conta verificada na Meta Business + Permanent Access Token. Sem janela de 24h só dá pra mandar <strong>templates aprovados</strong>. Anexos via Cloud API ficam para a Phase F.2.',
    'cloud.token.label': 'Access Token (não é exibido depois de salvo)',
    'cloud.phoneId.label': 'Phone Number ID',
    'cloud.wabaId.label': 'WhatsApp Business Account ID (para templates)',
    'cloud.version.label': 'API version (opcional)',
    'btn.saveCloud': 'Salvar credenciais',
    'btn.testCloud': 'Testar conexão',

    // ── Contact modal ──────────────────────────────────────────
    'dlg.contact.new': 'Novo contato',
    'dlg.contact.edit': 'Editar contato',
    'contact.name.label': 'Nome',
    'contact.whatsapp.label': 'WhatsApp (E.164)',
    'contact.waBusiness.label': 'WhatsApp Business',
    'contact.telegram.label': 'Telegram (@usuário ou telefone)',
    'contact.tags.label': 'Tags (separe por vírgula)',
    'contact.tags.placeholder': 'clientes-vip, família, …',
    'contact.notes.label': 'Notas',
    'btn.cancel': 'Cancelar',
    'btn.save': 'Salvar',
    'btn.ok': 'OK',

    // ── Debug modal ────────────────────────────────────────────
    'dlg.debug.title': 'Diagnóstico do DOM (topo do painel)',
    'dlg.debug.hint': 'Copie tudo abaixo e cole pro dev para que ele escreva o seletor correto.',
    'btn.copy': 'Copiar',
    'btn.close': 'Fechar',
    'debug.empty': '(vazio)',

    // ── Scrape modal ───────────────────────────────────────────
    'dlg.scrape.title': 'Resultado da raspagem',
    'scrape.selectAll': 'Selecionar todos',
    'scrape.filter.placeholder': 'Filtrar…',
    'scrape.col.name': 'Nome',
    'scrape.col.handle': 'Telefone / Username',
    'btn.scrapeImport': 'Importar selecionados',
    'scrape.dialog.title': 'Raspagem: {platform}',
    'scrape.dialog.found': '{n} conversa(s) encontrada(s). Selecione quais importar.',
    'scrape.noResults': 'Sem resultados',
    'scrape.nothingSelected': 'Nada selecionado',
    'scrape.labelPart': ' (etiqueta: {label})',
    'scrape.scraping': 'Raspando {name}{label}…',
    'scrape.cantScrape': 'Não foi possível raspar: {err}',
    'scrape.importResult': 'Raspagem: {added} novos · {merged} mesclados · {skipped} ignorados',
    'scrape.progress': 'Extraindo telefones… {current}/{total}',

    // ── Pick-list modal ────────────────────────────────────────
    'dlg.pickList.title': 'Adicionar à lista',
    'btn.add': 'Adicionar',

    // ── Workspace pickers ──────────────────────────────────────
    'scrape.workspace.none': '— nenhum WhatsApp/Telegram adicionado —',
    'campaign.workspace.none': '— nenhum {platform} adicionado em BigBox —',
    'workspace.noneInBigbox': 'Nenhum {platform} adicionado em BigBox. Adicione na barra lateral primeiro.',

    // ── Toasts / confirms / prompts ────────────────────────────
    'toast.error': 'Erro: {err}',
    'toast.stateLoadError': 'Erro carregando estado: {err}',
    'toast.contactSaved': 'Contato salvo',
    'confirm.deleteContact': 'Apagar este contato?',
    'toast.contactDeleted': 'Contato apagado',
    'toast.csvResult': 'CSV: {added} novos · {merged} mesclados · {skipped} ignorados',
    'toast.csvError': 'Erro no CSV: {err}',
    'confirm.bulkDelete': 'Apagar {n} contato(s)?',
    'toast.bulkDeleted': '{n} apagado(s)',
    'prompt.bulkTag': 'Aplicar tag a {n} contato(s):',
    'toast.tagApplied': 'Tag aplicada',
    'prompt.bulkUntag': 'Remover tag de {n} contato(s):',
    'toast.tagRemoved': 'Tag removida',
    'toast.createListFirst': 'Crie uma lista primeiro na aba Listas',
    'toast.addedToList': '{n} adicionado(s) à lista',
    'prompt.newList': 'Nome da nova lista:',
    'toast.listRenamed': 'Lista renomeada',
    'confirm.deleteList': 'Apagar lista "{name}"? (Os contatos permanecem.)',
    'toast.listDeleted': 'Lista apagada',
    'toast.maxDelayError': 'Atraso máximo deve ser ≥ mínimo',
    'toast.cloudSaved': 'Credenciais Cloud API salvas',
    'cloud.testing': 'testando…',
    'cloud.testOk': 'OK — {info}',
    'toast.copied': 'Copiado',
    'labels.error': 'Etiquetas: {err}',
    'labels.none': 'Nenhuma etiqueta encontrada',
    'labels.loaded': '{n} etiqueta(s) carregada(s)',
    'toast.selectAccountFirst': 'Selecione uma conta primeiro',
    'diag.error': 'Diag: {err}',
    'toast.addAccountFirst': 'Adicione um WhatsApp ou Telegram em BigBox primeiro.',
    'toast.templatesError': 'Erro listando templates: {err}',
    'toast.fileTooLarge': 'Arquivo grande demais (limite 64 MB)',
    'toast.loadingAttach': 'Carregando anexo…',
    'toast.attachReady': 'Anexo pronto',
    'err.selectList': 'selecione uma lista',
    'err.selectTag': 'selecione uma tag',
    'err.selectContacts': 'selecione contatos na aba Contatos',
    'err.invalidDate': 'data/hora inválida',
    'err.datePassed': 'a data agendada já passou',
    'err.selectTemplate': 'selecione um template aprovado',
    'toast.previewError': 'Erro no preview: {err}',
    'toast.emptyMessage': 'Mensagem vazia',
    'toast.noRecipients': 'Nenhum destinatário encontrado pra esta seleção',
    'toast.noHandles': 'Nenhum dos {count} contatos tem o campo "{field}" preenchido. Edite os contatos ou troque a plataforma.',
    'action.schedule': 'Agendar',
    'action.start': 'Iniciar',
    'when.at': ' em {datetime}',
    'when.now': ' agora',
    'confirm.startCampaign.skipped': ' ({n} serão pulados por falta de número)',
    'confirm.startCampaign': '{action} envio pra {count} destinatário(s){skipped}{when}?\nAtraso entre envios: {min}-{max}s.',
    'toast.campaignStarted': 'Campanha iniciada',
    'confirm.abort': 'Abortar campanha?',
    'toast.controlSent': 'Comando {action} enviado',
    'toast.autoPaused': 'Campanha pausada após {n} falhas seguidas',
    'toast.dailyCapReached': 'Limite diário atingido — campanha pausada',
  },

  'en': {
    // ── Header / tabs ──────────────────────────────────────────
    'app.subtitle': 'bulk messaging for WhatsApp & Telegram',
    'tab.contacts': 'Contacts',
    'tab.lists': 'Lists',
    'tab.campaign': 'Campaign',
    'tab.logs': 'Logs',
    'tab.settings': 'Settings',

    // ── Contacts toolbar ───────────────────────────────────────
    'contacts.search.placeholder': 'Search by name, phone or tag…',
    'contacts.tagFilter.all': 'All tags',
    'scrape.workspace.title': 'Account to scrape',
    'scrape.label.title': 'Filter by label before scraping',
    'scrape.label.none': '— no label filter —',
    'btn.refreshLabels.title': 'Load labels for this account',
    'btn.debugDom.title': 'Diagnose the panel-top DOM (paste the result to the dev)',
    'btn.scrape': 'Scrape',
    'btn.scrape.title': 'Scrape chats from the selected account',
    'btn.newContact': '+ New contact',
    'btn.importCsv': 'Import CSV',

    // ── Bulk bar ───────────────────────────────────────────────
    'bulk.count': '{n} selected',
    'btn.bulkTag': 'Apply tag…',
    'btn.bulkUntag': 'Remove tag…',
    'btn.bulkAddList': 'Add to list…',
    'btn.bulkDelete': 'Delete',

    // ── Contacts table ─────────────────────────────────────────
    'contacts.col.name': 'Name',
    'contacts.col.whatsapp': 'WhatsApp',
    'contacts.col.waBusiness': 'WA Business',
    'contacts.col.telegram': 'Telegram',
    'contacts.col.tags': 'Tags',
    'contacts.empty': 'No contacts yet. Add one manually, import a CSV, or scrape the account selected above (the account must be open and logged in to BigBox).',
    'contacts.tagsHint': '<strong>Tags:</strong> labels you create. Edit a contact and fill in the <em>Tags</em> box (comma-separated), or select several contacts and use <em>Apply tag…</em> in the bar above. Filter by tag in the dropdown next to the search, or use “By tag” in the <strong>Campaign</strong> tab.',
    'btn.edit': 'Edit',
    'btn.delete': 'Delete',

    // ── Lists ──────────────────────────────────────────────────
    'btn.newList': '+ New list',
    'lists.empty': 'No lists yet. Lists group contacts together for campaign sends.',
    'list.name.placeholder': 'List name',
    'btn.renameList': 'Rename',
    'btn.deleteList': 'Delete list',
    'list.members.heading': 'Members',
    'list.add.heading': 'Add contacts',
    'list.add.search.placeholder': 'Search contacts to add…',
    'list.member.remove': 'Remove',
    'list.add.add': 'Add',
    'list.empty.members': 'Empty list',
    'list.add.noCandidates': 'No candidates',
    'list.meta.count': '{n} contact(s)',

    // ── Campaign ───────────────────────────────────────────────
    'campaign.heading': 'New campaign',
    'campaign.name.label': 'Campaign name',
    'campaign.name.placeholder': 'e.g. Birthday promo',
    'campaign.platform.label': 'Platform',
    'platform.whatsapp_web': 'WhatsApp (Web)',
    'platform.whatsapp_business_web': 'WhatsApp Business (Web)',
    'platform.telegram': 'Telegram',
    'platform.whatsapp_cloud_api': 'WhatsApp Cloud API (Meta)',
    'campaign.workspace.label': 'Account (workspace) — which BigBox tab will send',
    'campaign.target.label': 'Recipients',
    'campaign.target.list': 'By saved list',
    'campaign.target.tag': 'By tag',
    'campaign.target.adhoc': 'Contacts selected in the Contacts tab',
    'campaign.list.label': 'List',
    'campaign.list.none': '— no lists —',
    'campaign.tag.label': 'Tag',
    'campaign.tag.none': '— no tags —',
    'campaign.adhoc.pre': 'Use the <strong>Contacts</strong> tab to select recipients via the checkboxes.',
    'campaign.adhoc.post': 'selected.',
    'campaign.cloud.msgType.label': 'Message type (Cloud API)',
    'campaign.cloud.msgType.template': 'Approved template (recommended)',
    'campaign.cloud.msgType.freeform': 'Free text (only works within the 24h window)',
    'campaign.template.label': 'Template',
    'campaign.body.label': 'Message — variables: <code>{nome}</code>, <code>{firstname}</code>, <code>{whatsapp}</code>, <code>{telegram}</code>, <code>{tag}</code>, <code>{notes}</code>',
    'campaign.body.placeholder': 'Hi {firstname}, …',
    'btn.emoji': '😀 Emojis',
    'btn.emoji.title': 'Insert emoji',
    'campaign.emoji.hint': 'or use the system shortcut (KDE: <kbd>Meta+.</kbd>, GNOME: <kbd>Ctrl+;</kbd>)',
    'campaign.attach.label': 'Attachment (optional — image, video, document or audio)',
    'btn.pickAttachment': '+ Attach file',
    'campaign.schedule.label': 'Schedule send (optional — leave blank to start now)',
    'btn.preview': 'Preview',
    'btn.start': 'Start sending',
    'btn.start.schedule': 'Schedule send',
    'campaign.recent.heading': 'Recent campaigns',
    'template.noParams': 'Template has no parameters.',
    'template.paramsHint': 'Template parameters — use variables like <code>{firstname}</code> or fixed text:',
    'template.param': 'Param {i}',
    'template.none': '— no APPROVED template found —',
    'template.loadError': '— failed to load —',
    'attach.remove': 'remove',
    'campaign.unnamed': 'Unnamed campaign',
    'campaign.recent.none': '— no campaigns yet —',
    'campaign.recent.meta': '{platform} · {n} send(s)',

    // ── Campaign preview ───────────────────────────────────────
    'preview.recipients': 'Recipients',
    'preview.withHandle': 'With an identifier for {platform}',
    'preview.missingHandle': 'Without an identifier (will be skipped)',
    'preview.dailyRemaining': 'Daily allowance remaining',
    'preview.warn': '⚠ More than {threshold} recipients. Bulk sending through WhatsApp Web can get your <strong>account banned</strong>. Consider smaller lists or a longer delay in Settings.',

    // ── Logs ───────────────────────────────────────────────────
    'logs.pause': 'Pause',
    'logs.resume': 'Resume',
    'logs.abort': 'Abort',
    'logs.col.when': 'When',
    'logs.col.contact': 'Contact',
    'logs.col.status': 'Status',
    'logs.col.error': 'Error',
    'logs.empty': 'Select a campaign above to see its progress.',
    'logs.option.select': '— select a campaign —',
    'logs.summary.total': 'Total:',
    'logs.summary.sent': '✓ Sent:',
    'logs.summary.failed': '✗ Failed:',
    'logs.summary.invalid': '# Invalid number:',
    'logs.summary.skipped': '↷ Skipped:',
    'logs.noSends': 'No sends yet',

    // ── Settings ───────────────────────────────────────────────
    'settings.heading': 'Sending settings',
    'settings.hint': 'The <em>defaults</em> are conservative to reduce the risk of a WhatsApp ban.',
    'settings.minDelay': 'Minimum delay between sends (seconds)',
    'settings.maxDelay': 'Maximum delay between sends (seconds)',
    'settings.dailyCap': 'Daily cap per platform',
    'settings.warn': 'Warn when recipients >',
    'settings.autopause': 'Auto-pause after N consecutive failures',
    'settings.retries': 'Extra retries per recipient on failure',
    'btn.saveSettings': 'Save',
    'settings.saved': 'Saved ✓',
    'settings.language.label': 'Idioma / Language',
    'cloud.heading': 'WhatsApp Cloud API (Meta)',
    'cloud.hint': 'A verified Meta Business account + Permanent Access Token. Without the 24h window you can only send <strong>approved templates</strong>. Cloud API attachments are planned for Phase F.2.',
    'cloud.token.label': 'Access Token (not shown after saving)',
    'cloud.phoneId.label': 'Phone Number ID',
    'cloud.wabaId.label': 'WhatsApp Business Account ID (for templates)',
    'cloud.version.label': 'API version (optional)',
    'btn.saveCloud': 'Save credentials',
    'btn.testCloud': 'Test connection',

    // ── Contact modal ──────────────────────────────────────────
    'dlg.contact.new': 'New contact',
    'dlg.contact.edit': 'Edit contact',
    'contact.name.label': 'Name',
    'contact.whatsapp.label': 'WhatsApp (E.164)',
    'contact.waBusiness.label': 'WhatsApp Business',
    'contact.telegram.label': 'Telegram (@username or phone)',
    'contact.tags.label': 'Tags (comma-separated)',
    'contact.tags.placeholder': 'vip-clients, family, …',
    'contact.notes.label': 'Notes',
    'btn.cancel': 'Cancel',
    'btn.save': 'Save',
    'btn.ok': 'OK',

    // ── Debug modal ────────────────────────────────────────────
    'dlg.debug.title': 'DOM diagnostics (panel top)',
    'dlg.debug.hint': 'Copy everything below and paste it to the dev so they can write the correct selector.',
    'btn.copy': 'Copy',
    'btn.close': 'Close',
    'debug.empty': '(empty)',

    // ── Scrape modal ───────────────────────────────────────────
    'dlg.scrape.title': 'Scrape result',
    'scrape.selectAll': 'Select all',
    'scrape.filter.placeholder': 'Filter…',
    'scrape.col.name': 'Name',
    'scrape.col.handle': 'Phone / Username',
    'btn.scrapeImport': 'Import selected',
    'scrape.dialog.title': 'Scrape: {platform}',
    'scrape.dialog.found': '{n} chat(s) found. Select which ones to import.',
    'scrape.noResults': 'No results',
    'scrape.nothingSelected': 'Nothing selected',
    'scrape.labelPart': ' (label: {label})',
    'scrape.scraping': 'Scraping {name}{label}…',
    'scrape.cantScrape': 'Could not scrape: {err}',
    'scrape.importResult': 'Scrape: {added} new · {merged} merged · {skipped} skipped',
    'scrape.progress': 'Extracting phones… {current}/{total}',

    // ── Pick-list modal ────────────────────────────────────────
    'dlg.pickList.title': 'Add to list',
    'btn.add': 'Add',

    // ── Workspace pickers ──────────────────────────────────────
    'scrape.workspace.none': '— no WhatsApp/Telegram added —',
    'campaign.workspace.none': '— no {platform} added in BigBox —',
    'workspace.noneInBigbox': 'No {platform} added in BigBox. Add it in the sidebar first.',

    // ── Toasts / confirms / prompts ────────────────────────────
    'toast.error': 'Error: {err}',
    'toast.stateLoadError': 'Error loading state: {err}',
    'toast.contactSaved': 'Contact saved',
    'confirm.deleteContact': 'Delete this contact?',
    'toast.contactDeleted': 'Contact deleted',
    'toast.csvResult': 'CSV: {added} new · {merged} merged · {skipped} skipped',
    'toast.csvError': 'CSV error: {err}',
    'confirm.bulkDelete': 'Delete {n} contact(s)?',
    'toast.bulkDeleted': '{n} deleted',
    'prompt.bulkTag': 'Apply tag to {n} contact(s):',
    'toast.tagApplied': 'Tag applied',
    'prompt.bulkUntag': 'Remove tag from {n} contact(s):',
    'toast.tagRemoved': 'Tag removed',
    'toast.createListFirst': 'Create a list first in the Lists tab',
    'toast.addedToList': '{n} added to the list',
    'prompt.newList': 'New list name:',
    'toast.listRenamed': 'List renamed',
    'confirm.deleteList': 'Delete list "{name}"? (Contacts are kept.)',
    'toast.listDeleted': 'List deleted',
    'toast.maxDelayError': 'Maximum delay must be ≥ minimum',
    'toast.cloudSaved': 'Cloud API credentials saved',
    'cloud.testing': 'testing…',
    'cloud.testOk': 'OK — {info}',
    'toast.copied': 'Copied',
    'labels.error': 'Labels: {err}',
    'labels.none': 'No labels found',
    'labels.loaded': '{n} label(s) loaded',
    'toast.selectAccountFirst': 'Select an account first',
    'diag.error': 'Diag: {err}',
    'toast.addAccountFirst': 'Add a WhatsApp or Telegram in BigBox first.',
    'toast.templatesError': 'Error listing templates: {err}',
    'toast.fileTooLarge': 'File too large (64 MB limit)',
    'toast.loadingAttach': 'Loading attachment…',
    'toast.attachReady': 'Attachment ready',
    'err.selectList': 'select a list',
    'err.selectTag': 'select a tag',
    'err.selectContacts': 'select contacts in the Contacts tab',
    'err.invalidDate': 'invalid date/time',
    'err.datePassed': 'the scheduled date has already passed',
    'err.selectTemplate': 'select an approved template',
    'toast.previewError': 'Preview error: {err}',
    'toast.emptyMessage': 'Empty message',
    'toast.noRecipients': 'No recipients found for this selection',
    'toast.noHandles': 'None of the {count} contacts has the "{field}" field filled in. Edit the contacts or switch platform.',
    'action.schedule': 'Schedule',
    'action.start': 'Start',
    'when.at': ' on {datetime}',
    'when.now': ' now',
    'confirm.startCampaign.skipped': ' ({n} will be skipped for a missing number)',
    'confirm.startCampaign': '{action} sending to {count} recipient(s){skipped}{when}?\nDelay between sends: {min}-{max}s.',
    'toast.campaignStarted': 'Campaign started',
    'confirm.abort': 'Abort campaign?',
    'toast.controlSent': 'Command "{action}" sent',
    'toast.autoPaused': 'Campaign paused after {n} consecutive failures',
    'toast.dailyCapReached': 'Daily cap reached — campaign paused',
  },
};

// Resolve the active language: stored choice → system language → fallback.
function resolveInitialLang() {
  try {
    const stored = localStorage.getItem(LANG_STORAGE_KEY);
    if (stored && SUPPORTED_LANGS.includes(stored)) return stored;
  } catch (_) {}
  const sys = (navigator.language || navigator.userLanguage || '').toLowerCase();
  if (sys.startsWith('pt')) return 'pt-BR';
  if (sys.startsWith('en')) return 'en';
  return FALLBACK_LANG;
}

let LANG = resolveInitialLang();

function currentLang() {
  return LANG;
}

// Translate `key`, interpolating `{var}` tokens only when `vars` is given.
function t(key, vars) {
  const dict = I18N[LANG] || I18N[FALLBACK_LANG];
  let v = dict[key];
  if (v === undefined) v = I18N[FALLBACK_LANG][key];
  if (v === undefined) v = key; // last resort: show the key, never crash
  if (vars) {
    v = v.replace(/\{(\w+)\}/g, (m, k) => (vars[k] !== undefined ? vars[k] : m));
  }
  return v;
}

// Fill every element carrying a data-i18n* attribute. Safe to call repeatedly.
function applyStaticI18n(root = document) {
  root.querySelectorAll('[data-i18n]').forEach(el => {
    el.textContent = t(el.getAttribute('data-i18n'));
  });
  root.querySelectorAll('[data-i18n-html]').forEach(el => {
    el.innerHTML = t(el.getAttribute('data-i18n-html'));
  });
  root.querySelectorAll('[data-i18n-placeholder]').forEach(el => {
    el.setAttribute('placeholder', t(el.getAttribute('data-i18n-placeholder')));
  });
  root.querySelectorAll('[data-i18n-title]').forEach(el => {
    el.setAttribute('title', t(el.getAttribute('data-i18n-title')));
  });
  document.documentElement.lang = LANG;
}

// Persist + apply a new language. `onChange` lets studio.js re-render the
// dynamic (JS-generated) parts of the UI after the static pass.
function setLang(lang, onChange) {
  if (!SUPPORTED_LANGS.includes(lang)) return;
  LANG = lang;
  try { localStorage.setItem(LANG_STORAGE_KEY, lang); } catch (_) {}
  applyStaticI18n();
  if (typeof onChange === 'function') onChange();
}
