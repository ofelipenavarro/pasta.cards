# Mapa de Migração: Tauri/JS/Python → PLEV

> Levantamento exaustivo do estado atual de `pasta.cards` e proposta de mapeamento para a UI nativa PLEV.
> Última auditoria completa: 2026-08-30.

## 1. Resumo da arquitetura atual
cd
   /Users/nn/Dev/pasta.cards/desktop/src-taur i && cargo run 2>&1

| Camada | Antigo (Tauri/JS/Python) | Novo (PLEV) |
|--------|--------------------------|-------------|
| UI | HTML/CSS/JS em `desktop/ui/js` + Tauri webview | Rust nativo `crates/spellbook` com `engine` (Hoff/plev) |
| Comunicação | Servidor HTTP Axum embutido (`/api/*`) | Thread worker `SpellbookClient` via `mpsc` (`Command`/`Event`) |
| Backend dados | Rust Tauri (`desktop/src-tauri/src`) | Rust puro `crates/spellbook-core` |
| CLI auxiliar | `edhrec.py`, `mtgdb.py`, `scryfall.py` | Mantidos como CLI; dados agora consumidos pelo core |
| Banco | `app.db` + `mtg.sqlite` | Mesmos arquivos, schema preservado |
| Cache imagens | `data/images/` | Mesmo diretório, lido por `ArtCache` via `LoadArt`/`ArtLoaded` |

## 2. Mapeamento de telas / rotas

| Rota antiga (hash) | View JS | Tela PLEV atual | Status | Componentes PLEV necessários |
|--------------------|---------|-----------------|--------|------------------------------|
| `#dashboard` | `views/home.js` | `view/home.rs` | ✅ Funcional | Estatísticas, grid de decks, atividades |
| `#decks` | `views/decks.js` | `view/decks.rs` | ❌ Placeholder | Grid, modais novo/editar/excluir/importar |
| `#deck/:id` | `views/deck-detail.js` | `view/decks.rs` (placeholder) | ❌ Placeholder | Detalhe, filtros, views, sinergia, curva |
| `#collection` | `views/collection.js` | `view/collection.rs` | ❌ Placeholder | Grid, filtros, modal de adicionar/editar |
| `#wishlist` | `views/wishlist.js` | `view/wishlist.rs` | ❌ Placeholder | Grid, filtros, ações comprar/remover |
| `#scanner` | `views/scanner.js` | `view/scanner.rs` | ❌ Placeholder permanente | Apenas aviso "Em breve" |
| `#games` | `views/games.js` | `view/games.rs` | ❌ Placeholder | Lista, estatísticas, modal registrar |

## 3. Mapeamento de componentes UI

| Componente JS (`desktop/ui/js/ui/`) | Componente/funcionalidade PLEV | Status |
|-------------------------------------|--------------------------------|--------|
| `add-card.js` | `view/components/add_card.rs` | ✅ Implementado (single + lista) |
| `card-modal.js` | `view/components/card_modal.rs` | ✅ Implementado (detalhe + cópias) |
| `card-face.js` | Renderização inline em `card_modal.rs` | ⚠️ Parcial (sem flip explícito ainda) |
| `set-picker.js` | `view/components/set_picker.rs` | ✅ Implementado |
| `search-field.js` | `view/components/search_field.rs` | ⚠️ Implementado mas com erros de integração |
| `confirm.js` | `view/components/confirm.rs` + `engine::ui::widgets::Modal` | ⚠️ Implementado mas não usado; bug em `take_result` |
| `card-filters.js` | **Ainda não existe** em PLEV | ❌ Necessário para collection/deck/wishlist |
| `deck-bits.js` | `view/deck_tile.rs` | ✅ Parcial (tile usado na home) |
| `icons.js` | `engine::ui::icons` + helpers locais | ⚠️ Parcial (falta mana cost glyphs) |
| `sidebar.js` | `view/mod.rs` (rail + footer) | ⚠️ Implementado rail; falta painel de dados |
| `util.js` | Helpers em `view/mod.rs` | ⚠️ Parcial (falta `pollJob`, `formatTs`) |

## 4. Mapeamento de endpoints → Commands/Events

### Cobertos diretamente

| Endpoint antigo (`/api/*`) | Command PLEV | Event PLEV | Backend core |
|---------------------------|--------------|------------|--------------|
| `GET /decks` | `ListDecks` | `DecksListed` | ✅ |
| `GET /decks/:id` | `GetDeck { deck_id }` | `DeckLoaded` | ✅ |
| `POST /decks` | `CreateDeck(...)` | `DeckCreated` | ✅ |
| `PUT /decks/:id` | `UpdateDeck { ... }` | `DeckUpdated` | ✅ |
| `DELETE /decks/:id?mode=` | `DeleteDeck { ... }` | `DeckDeleted` | ✅ |
| `POST /decks/:id/cards` | `AddDeckCard { ... }` | `DeckCardAdded` | ✅ |
| `DELETE /decks/:deck_id/cards/:card_id` | `RemoveDeckCard { ... }` | `DeckCardRemoved` | ✅ |
| `POST /decks/:id/import/preview` | `ImportPreview { ... }` | `ImportPreviewed` | ✅ |
| `POST /decks/:id/import/commit` | `ImportCommit { ... }` | `ImportCommitted` | ✅ |
| `POST /decks/auto-build` | `AutoBuild(...)` | `AutoBuildFinished` | ✅ |
| `GET /decks/auto-build/status` | `AutoBuildStatus` | `AutoBuildStatus` | ✅ |
| `GET /decks/:id/synergy` | `DeckSynergy { ... }` | `SynergyLoaded` | ✅ |
| `POST /decks/:id/synergy/fetch` | `FetchDeckSynergy { ... }` | `Failed`/`SynergyLoaded` | ✅ |
| `GET /cards/search` | `SearchCards { ... }` | `CardsFound` | ✅ |
| `GET /cards/:name` | `GetCard { ... }` | `CardLoaded` | ✅ |
| `GET /cards/:name/printings` | `CardPrintings { ... }` | `PrintingsLoaded` | ✅ |
| `GET /cards/:name/variants` | `CardVariants { ... }` | `VariantsLoaded` | ✅ |
| `GET /sets` | `SearchSets { ... }` | `SetsFound` | ✅ |
| `GET /collection` | `ListCollection { ... }` | `CollectionListed` | ✅ |
| `GET /collection/copies` | `CardCopies { ... }` | `CardCopiesLoaded` | ✅ |
| `POST /collection` | `AddCollection(...)` | `CollectionAdded` | ✅ |
| `PATCH /collection/:id` | `EditCollection { ... }` | `CollectionEdited` | ✅ |
| `DELETE /collection/:id` | `DeleteCollection { ... }` | `CollectionDeleted` | ✅ |
| `PATCH /collection/:id/allocate` | `AllocateCollection { ... }` | `CollectionAllocated` | ✅ |
| `GET /wishlist` | `ListWishlist { ... }` | `WishlistListed` | ✅ |
| `POST /wishlist` | `AddWishlist(...)` | `WishlistAdded` | ✅ |
| `DELETE /wishlist/:id` | `DeleteWishlist { ... }` | `WishlistDeleted` | ✅ |
| `POST /wishlist/:id/acquire` | `AcquireWishlist { ... }` | `WishlistAcquired` | ✅ |
| `GET /games` | `ListGames` | `GamesListed` | ✅ |
| `GET /games/stats` | `GamesStats` | `GamesStatsLoaded` | ✅ |
| `POST /games` | `AddGame(...)` | `GameAdded` | ✅ |
| `GET /data/info` | `DataInfo` | `DataInfoLoaded` | ✅ |
| `POST /data/update` | `UpdateStart` | `UpdateStarted` | ✅ |
| `GET /data/update/status` | `UpdateStatus` | `UpdateStatusLoaded` | ✅ |
| `POST /data/update/cancel` | `UpdateCancel` | *(nenhum evento)* | ✅ |
| `GET /data/images` | `ImagesInfo` | `ImagesInfoLoaded` | ✅ |
| `GET /img/*rel` | `LoadArt { rels, max_edge }` | `ArtLoaded` | ✅ |

### Agrupados / parcialmente cobertos

| Endpoint antigo | Situação no novo core |
|-----------------|------------------------|
| `GET /collection/total` | Não há Command/Event isolado. Valores vêm dentro de `HomeData` via `LoadHome` / `HomeLoaded`. |
| `GET /wishlist/total` | Não há Command/Event isolado. Valores vêm dentro de `HomeData` via `LoadHome` / `HomeLoaded`. |
| `GET /activity` | Não há Command/Event isolado. `LoadHome` traz `activity` com `limit = 20`; não há como pedir outro limite. |
| `GET /decks/:id/tags` | Função `ops::decks::deck_tags` existe internamente, mas **não há** `Command`/`Event` para chamá-la. |

### NÃO implementados

| Endpoint antigo | Impacto |
|-----------------|---------|
| `GET /decks/:id/export?format=` | Botão Exportar do deck detail não funciona |
| `GET /collection/duplicates` | Tela de coleção não mostra duplicatas |
| `POST /collection/bulk-resolve` | Modal "Adicionar por lista" não funciona |
| `POST /scan/recognize` | Scanner continua placeholder |

## 5. Gaps identificados

### Backend (`spellbook-core`)

| Gap | Impacto | Prioridade |
|-----|---------|------------|
| `POST /collection/bulk-resolve` (adicionar por lista) | Modal "Adicionar por lista" não funciona no PLEV | Alta |
| `GET /collection/duplicates` | Tela de coleção não mostra duplicatas | Média |
| `GET /decks/:id/export` (Moxfield/texto) | Botão Exportar do deck detail não funciona | Média |
| `GET /decks/:id/tags` não exposto via Command/Event | Agrupamento por "Subtipo" no deck detail inoperante | Média |
| `GET /activity` não exposto via Command/Event | Não é possível carregar feed de atividades com limite diferente de 20 | Baixa |
| `Command::ImagesInfo` existe mas não há UI para mostrar | Painel de dados da sidebar não mostra cache | Baixa |
| `POST /scan/recognize` | Scanner continua placeholder | Baixa |

### UI (`spellbook`)

| Gap | Impacto | Prioridade |
|-----|---------|------------|
| Telas `DecksScreen`, `CollectionScreen`, `WishlistScreen`, `GamesScreen` são placeholders | Apenas Home e modais funcionam | Alta |
| Sistema de filtros (`card-filters.js`) não existe | Coleção/Deck/Wishlist sem filtros | Alta |
| Modal de Novo/Editar/Excluir/Importar deck não existe | CRUD de decks inoperante | Alta |
| Modal de registrar partida não existe | Tela de partidas inoperante | Alta |
| `SearchField` tem erros de integração | Campo de busca não pode ser usado | Alta |
| `ConfirmDialog` não é usado e tem bug em `take_result` | Diálogos de confirmação inoperantes | Média |
| `AddCardModal` e `CardModal` não estão plugados em telas | Fluxo de adicionar/ver carta não acessível | Alta |
| Renderização de mana cost glyphs não existe | Detalhe da carta sem custo de mana visual | Média |
| `pollJob` / barra de progresso para updater/auto-build não existe | Jobs longos sem feedback visual | Média |
| Sidebar não mostra painel de dados/atualização | Perde funcionalidade da JS sidebar | Média |
| Visualização Empilhado (stack) de deck | Funcionalidade avançada de deck detail | Baixa |
| Sub-lista de decks na sidebar não existe | Navegação rápida para decks perdida | Média |

## 6. Plano de execução

### Fase 1 — Fundação e correções
- [x] Migrar comunicação HTTP para `Command`/`Event` via worker thread
- [x] Implementar shell global (sidebar, header, navegação, toasts, overlays)
- [x] Implementar componentes base: `ModalFrame`, `LabeledField`, `SetPicker`
- [x] Implementar modais `AddCardModal` e `CardModal`
- [ ] Corrigir `SearchField` para integrar corretamente com `LabeledField`
- [ ] Corrigir bug em `ConfirmDialog::take_result`
- [ ] Plugar `AddCardModal` e `CardModal` no shell global (atalho/tecla ou botão)
- [ ] Implementar helper `format_ts` e infra de `poll_job`

### Fase 2 — Coleção (alta prioridade)
- [ ] Implementar `CollectionScreen` com grid responsivo
- [ ] Integrar `AddCardModal` e `CardModal` na coleção
- [ ] Implementar sistema de filtros genérico (`FilterState`, `FilterDropdown`)
- [ ] Implementar ações de editar/deletar/alocar cópia
- [ ] Implementar `bulk-resolve` no backend (ou adaptar modal para usar `AddCollection` individual)

### Fase 3 — Decks
- [ ] Implementar `DecksScreen` (grid + tile "Novo Deck")
- [ ] Implementar modal Novo Deck (com autocomplete de comandante)
- [ ] Implementar modal Editar Deck
- [ ] Implementar modal Excluir Deck
- [ ] Implementar modal Importar Decklist
- [ ] Implementar `DeckDetailScreen` com:
  - Header, breadcrumb, estatísticas
  - Painel de ownership
  - Painel de sinergia EDHREC
  - Curva de mana
  - Toolbar (adicionar, visualizações, agrupar, filtrar, ordenar)
  - Lista/grid/stack de cartas

### Fase 4 — Wishlist e Partidas
- [ ] Implementar `WishlistScreen` com grid, filtros, ações
- [ ] Implementar `GamesScreen` com estatísticas e histórico
- [ ] Implementar modal Registrar Partida

### Fase 5 — Polimento
- [ ] Implementar painel de dados/updater na sidebar
- [ ] Implementar renderização de mana cost glyphs
- [ ] Implementar `pollJob` / barra de progresso
- [ ] Implementar exportação de deck
- [ ] Adicionar sub-lista de decks na sidebar
- [ ] Adicionar testes de UI headless

## 7. Decisões pendentes para o depara

1. **Adicionar por lista**: implementar `bulk-resolve` no backend ou fazer o modal iterar `AddCollection` no frontend?
2. **Filtros**: componente genérico compartilhado entre deck/coleção/wishlist (como no JS) ou um por tela?
3. **Deck detail visualizações**: implementar as três visualizações (lista/grid/empilhado) agora ou começar só com grid/lista?
4. **Scanner**: manter placeholder permanente ou remover a rota?
5. **Mana glyphs**: usar fonte símbolo (ex: `Beleren`) ou desenhar SVGs inline como no JS?
6. **Sidebar data panel**: mostrar info do banco + botão update como no JS, ou mover para uma tela de configurações?
7. **Wishlist e add-card**: a wishlist deve continuar usando o modal `AddCardModal` (como no JS) ou ter fluxo próprio?

---

*Gerado em 2026-08-30. Baseado na análise de `desktop/ui/js`, `crates/spellbook-core`, `crates/spellbook/src/view`, `crates/engine` do PLEV, scripts Python e `desktop/src-tauri`.*
