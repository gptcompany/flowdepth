# Console operativa del sistema: specifica draft e piano

Stato: draft per B5, 2026-11-08. Decisione D4: esporre le viste mediante
il contratto flow-depth v1 di dash-core, come il max pain di deribit-options-analysis #10.
Questa proposta contiene solo documentazione; non installa servizi né modifica gli host.

## Obiettivo e scelta del repository

L'operatore deve capire dove intervenire, quali dati sono affidabili e quanto sono recenti,
senza aprire console separate. Il primo rilascio serve a operare, non a raccogliere tutte
le visualizzazioni possibili. Assunzione da confermare: console personale su Mac via WireGuard,
nessuna autenticazione aggiuntiva per il listener loopback; accesso remoto solo nella VPN.

Il proprietario è **gptcompany/flowdepth**, branch di integrazione **develop**:
contiene insieme `dash-core` (aggregatore, contratto e pagina web) e il consumatore desktop
`src/widget/sheet.rs`. `main` è ancora la linea upstream precedente all'integrazione.
Mettere la specifica in nautilus_dev replicherebbe il proprietario della console;
metterla in gbg-agents separerebbe contratto e manutenzione.
Il repository non ha una cartella/template spec: si segue il formato Markdown dei documenti
in `docs/`, mantenendo inventario, requisiti e piano nello stesso documento.

## Riuso già disponibile

- Desktop Rust/iced: candele, footprint, heatmap L2, volume bubbles, iceberg;
  REST storico, WebSocket live, cache e riconnessione degli adapter exchange esistenti.
  Le fonti includono Binance, Bybit, Hyperliquid, MEXC e OKX, con supporto diverso per vista.
- Il desktop ha già un grafico GEX e connector opzioni Deribit/Derive e altre fonti;
  il flusso maker Derive è distinto dal GEX. Riutilizzare dopo accordo sulla metodologia.
- dash-core raccoglie in parallelo i fogli `infra`, `alerts`, `rektslug`, `completeness`,
  `shadow`; espone `/` e `GET /v1/sheets`. Una fonte indisponibile produce righe missing
  con HTTP 200, non la perdita degli altri fogli. Log JSON: durata e numero righe.
- Il widget Sheet ordina crit > warn > stale > missing > ok, conserva la tab selezionata
  e mantiene i dati con banner in caso di errore. Poll desktop 5 s, timeout 4 s;
  pagina web poll 30 s. Il collector ha timeout 5 s per richiesta e nessuna cache:
  alcune raccolte possono superare il timeout del consumatore.
- Deploy già versionato: `dash-core/deploy/render-plist.py`, launchd utente sul Mac,
  binary per SHA, symlink e rollback; listener predefinito `127.0.0.1:8790`.
  README registra verifica Mac del 26/10; non è una verifica live del Mac oggi.
  Nessun nuovo processo per il progetto: estendere quello esistente.
- nautilus_dev ha già dashboard FastAPI e modelli Cockpit: riusare la semantica
  evidence/runtime, senza copiare il frontend né dichiararlo sano da un semplice `up`.

## Inventario delle fonti e limiti

Latenze sotto: budget proposti, salvo le misure esplicitamente datate. Le cadenze di scrape,
producer e conservazione vanno lette dalla configurazione distribuita prima di fissare SLA.
Nessun endpoint interno diventa pubblico per effetto di questa specifica.

| Vista | Fonte/endpoint o metrica già esistente | Freschezza e accesso |
| --- | --- | --- |
| Topologia essenziale | network-infra `ansible/inventory/hosts.yml`; Prometheus `up`, `kube_node_info`, `kube_pod_info`, `wireguard_peer_last_handshake_seconds` | Inventory al deploy; metriche al relativo scrape. Backend via WG; nessun kubeconfig o chiave WG nel browser. |
| Alert attivi | Prometheus `GET /api/v1/alerts`, regole monitoring-stack | Poll 30 s oltre valutazione/scrape; endpoint WG già usato. `activeAt` è età dell'alert, non ultima acquisizione. |
| Silenzi/inibizioni | Alertmanager `GET /api/v2/alerts` | Accesso interno da verificare; non necessario per MVP. Prometheus da solo non prova consegna né silenzio. |
| Notifiche | monitoring-stack alertbus/ntfy; `alertmanager_notifications_total`, `alertmanager_notifications_failed_total`; receiver GitHub `/metrics` | Poll 30 s; riusare metriche esistenti. Nessuna lettura di topic, token, contenuti ntfy o nuove sottoscrizioni. |
| Servizi | Prometheus `up`, `node_systemd_unit_state`; textfile di esito/ultimo successo già prodotti | Poll 30 s; distinguere scrape, unità persistente, timer e oneshot. Nessun SSH dal browser. |
| Qualità HL | `hl_block_continuity_*`, `hl_core_block_height`, `hl_core_latest_block_time`; quality map JSON `sync/quality_map.py` | Metriche già nel foglio completeness. Mappe con `generated_at` e hash; endpoint HTTP assente, deploy producer non dimostrato. |
| Rektslug stato | `GET /health`, `/ops/summary`, `details.data_freshness` per tabella/simbolo | Usare `data_as_of` e `sla_secs` della fonte, fallback attuale 600 s reso esplicito. Backend locale/WG. |
| Heatmap sul prezzo | rektslug `GET /liquidations/heatmap-timeseries`, `/prices/klines`; legacy `/liquidations/heatmap`; WS già presenti | Richieste limitate per simbolo/intervallo; `data_as_of`, `stale`, `X-Data-As-Of`, `X-Data-Stale`. Nessuno SLA live ancora misurato. |
| Max pain | deribit-options-analysis CLI `--format flowdepth`, foglio `maxpain` | Output JSON v1 già pronto, non server HTTP; cadenza e collocazione artifact non verificate. Import solo backend in dash-core. |
| GEX/pressione opzioni | Grafico/connector GEX già nel desktop; deribit-options-analysis oggi implementa max pain | Nessun endpoint GEX da inventare in quel package. Dopo max pain e chiarimento fonte/metodologia. |
| Nautilus Cockpit | FastAPI `/api/portfolio/cockpit`, `/evidence`, `/runtime-snapshot`, `/runtime-ledger`; `/api/status` | Budget futuro 30 s; snapshot obbligatorio fail closed. Reachability del servizio non verificata positivamente oggi. |
| Shadow tk-nt-1 | `nautilus_shadow_orders_total`, `fills_total`, `blocked_without_id_total`, `realized_pnl_usd`, `last_order_timestamp_seconds`, `exporter_run_timestamp_seconds` con prefisso comune `nautilus_shadow_` | Exporter ledger timer ogni minuto; poll 30 s. Timestamp exporter >15 min stale; PNL attuale non affidabile F033/F035. |

Gli URL runtime vengono dalla configurazione backend esistente (`DASH_PROM_URL`,
`DASH_REKTSLUG_URL`), non da input libero della pagina. Non leggere file credenziali per
completare l'inventario. Indirizzi e ruoli appartengono a inventory: riferimenti storici
all'IP del monitor non sono una seconda configurazione da copiare.

### Semantica da preservare

- Infra attuale considera ogni valore HL finito ok: non equivale a una soglia di qualità.
  `up=1` significa scrape riuscito, non servizio utilizzabile o dati completi.
- Alert pending resta warn anche con etichetta critical; firing usa severità della regola.
  Un alert firing da giorni non diventa stale: stale riguarda l'acquisizione della fonte.
- Quality map v1: index `generated_at`, giorni con file e SHA256; mappe per stream/simbolo,
  intervalli UTC di ore 0..23 inclusivi `present`, `missing`, `unrecoverable`, `single_source`.
  Missing locale non significa irrecuperabile; single_source non va contato due volte.
  La review #350 separa libreria/schema dal deploy: senza artifact mostrare indisponibile.
- Cockpit: `PortfolioCockpitResponse` espone registry/runtime caricati, `cockpit_ready`,
  `blocking_cards`, `generated_at`, cards con stato, evidenze e azione richiesta.
  Evidence registry da solo non prova runtime vivo; heartbeat snapshot è la freschezza.
  `/api/status.last_fetch` può essere ora anche con eventi vecchi: non usarlo come SLA.
- Shadow: baseline mezzanotte UTC e reset counter già gestiti da dash-core; non sommare
  stati ordine, che rappresentano fasi dello stesso ordine. Timer attivo e oneshot inattivo
  sono normali. Non promuovere PNL non affidabile a indicatore decisionale.
- Heatmap rektslug rappresenta esposizione stimata a liquidazioni, distinta da liquidazioni
  eseguite e dalla heatmap L2. Il prezzo deve avere simbolo, venue, quote, UTC e scala coerenti.
  Non sovrapporre automaticamente una griglia Binance a candele Hyperliquid.

## MVP: operare con i fogli esistenti

1. Alert: firing/pending, componente, severità, età dell'evento e freschezza acquisizione;
   link sicuro alla fonte quando disponibile, errore fonte distinto da assenza di alert.
2. Stato servizi: ruolo/host, scrape, unità/timer, ultimo esito noto; shadow incluso.
   Non aggiungere controlli di amministrazione, restart o esecuzione comandi.
3. Qualità dati: continuità HL e freshness rektslug già disponibili; quality map solo se
   artifact già presente e validato. Mostrare esplicitamente assenza e ambito coperto.
4. Topologia essenziale: tabella host/ruolo, peer WG e nodi/pod k3s osservati, collegamenti
   al servizio interessato. Inventory dichiarato e scoperta runtime restano distinguibili.
   Nessun editor grafico o discovery agente aggiuntivo.

Le notifiche nell'MVP sono un indicatore di errori di consegna esistenti; non un archivio
messaggi. Il Cockpit completo resta successivo: shadow e sue evidenze essenziali bastano
per il primo rilascio. La pagina web esistente è fallback operativo del desktop.

## Contratto e sicurezza

MVP senza estensioni v1: `SheetsResponse { generated_at, sheets }`, ogni Sheet ha
`id`, `title`, `columns`, `rows`; colonne fisse `component,status,value,source,age_s`.
Ogni cella mantiene `value`, `severity`, `source`, `ts`; severità ammesse
`ok,warn,crit,stale,missing`. ID stabili, nessuna rimozione/riorganizzazione dei fogli
esistenti; righe nuove compatibili, unità e origine leggibili nella colonna value/source.
`generated_at` è raccolta della risposta, `ts` è timestamp pertinente alla cella;
non sostituire timestamp ignoti con ora. `age_s` non misura l'età di activeAt come freshness.

Il backend valida input e artifact con parser/schema maturi; limiti a dimensione, tempi,
simboli e finestre temporali. Mantiene allowlist delle origini configurate e rifiuta URL
arbitrari/redirect fuori perimetro. Nessuna credenziale, chiave, topic o URL con query
sensibile nella risposta o nei log: usare alias sorgente. La redazione backend attuale
della sola userinfo non basta a garantire questo requisito; serve verifica dedicata.
Nessuna credenziale nel browser/local storage. Nessuna nuova porta pubblica, CORS aperto,
scrittura remota, accesso al socket systemd o kubeconfig condiviso. Loopback sul Mac;
se serve accesso da altri peer, bind WG esplicito e ACL/inventory nella configurazione unica.
Permessi di artifact solo lettura; nessun service account amministrativo per la console.

Per una futura overlay numerica, una tabella di stringhe è insufficiente: non incorporare
JSON di griglia nella cella value. Solo dopo la prova di riuso del chart nativo, introdurre
nel medesimo dash-core una risorsa GET separata (per esempio `/v1/overlays/liquidations`),
che adatti i modelli upstream con timestamp/prezzo/densità, metadata e stato stale.
Conservare `/v1/sheets` invariato e mostrare anche lì riepilogo/disponibilità dell'overlay.
La scelta precisa dell'endpoint richiede prima budget payload e prova di allineamento prezzo;
nessuna estensione numerica è approvata o necessaria per l'MVP.

## Prestazioni e osservabilità

Obiettivi da verificare prima del rilascio operativo, non misure già ottenute:
poll operazioni 30 s, cache ultimo risultato/coalescing nel processo esistente;
risposta cached p95 <250 ms locale e raccolta p95 <4 s con 3 client per 10 minuti.
Non sommare timeout per ogni client; una fonte lenta non deve bloccare gli altri fogli.
Dati vecchi rimangono visibili marcati stale con errore fonte, mai ripubblicati come nuovi.
Per heatmap partire con una sola vista/simbolo, finestra 24 h, intervallo 15 min,
limite risposta 2 MiB; misurare prima di introdurre WS o storico più grande.

Riusare tracing JSON e monitoring-stack. Registrare durata/esito/freschezza per collector
con label finite, senza simboli arbitrari o URL. Riutilizzare alert di fonte già esistenti;
un errore di visualizzazione non deve inviare nuove notifiche o aprire issue a ogni poll.
Una richiesta HTTP 200 non equivale a fonti sane: osservare missing/stale e ultimo successo.

## Criteri di accettazione verificabili

- A1: desktop e pagina `/` leggono la stessa risposta v1 e restano utilizzabili con un
  collector indisponibile; fixture di errore produce missing/stale, le altre viste persistono.
- A2: con `up=1` e timestamp dati scaduto la qualità resta stale; un firing vecchio resta
  firing. Test riproducibile sui collector con clock controllato, prima della modifica.
- A3: tabella distingue inventory assente, peer senza handshake, nodo/pod non osservato;
  non deduce rimozione dall'inventory da un timeout Prometheus.
- A4: su shadow confrontare timestamp exporter e ultimo ordine; un periodo senza trade
  non è exporter guasto. PNL non affidabile segnalato, counter reset e UTC preservati.
- A5: artifact qualità mancante, checksum errato e schema invalido non diventano ok;
  nessuna mappa mostrata come attuale in base al solo timestamp della risposta HTTP.
- A6: ispezione risposta, log e bundle non trova topic/credenziali; test con valori sentinella
  artificiali in userinfo/query verifica redazione. Probe da fuori WG non raggiunge listener.
- A7: benchmark con tre client rispetta budget e isolamento sopra; numero richieste upstream
  non triplica. Nessun build/test sul Dell: evidenza riproducibile nella CI appropriata.
- A8, fase overlay: fixture con prezzo noto verifica assi UTC, bin, unità, venue e stale;
  cambio simbolo/intervallo cancella dati precedenti, fonte incoerente rende overlay indisponibile.

## Piano per PR piccole e ordine di ROI

Stime: una modifica coerente e circa 300 righe per PR, inclusi test; ogni passo può essere
suddiviso se la prova supera il budget. Nessuna nuova shell: eventuale glue usa Python.
La spec non autorizza implementazioni né dichiara già passate le verifiche future.

| Ordine | Costo stimato | Riuso e beneficio | Passo → verifica prima del merge |
| --- | --- | --- | --- |
| 0 | Questa PR docs | Fissa perimetro, fonti e proprietario | Diff solo Markdown, link ai file base, CI develop verde. |
| 1 | 1–2 PR | Collector/cache, errori e freshness esistenti: console affidabile | Prima regressioni A1/A2 e redazione A6, poi modifica; `cargo test -p dash-core --locked`, CI Develop CI. |
| 2 | 1–2 PR | up/systemd/textfile e shadow già integrati: decidere quale servizio controllare | Fixture A4, confronto read-only tk-nt-1 e timestamp Prom; CI test collector e widget. |
| 3 | 1–2 PR | Completeness/rektslug e quality map se disponibile: evitare decisioni su dati cattivi | A5 con fixture artifact; confronto hash/timestamp del producer senza leggerne segreti; CI. Nessun deploy producer nascosto. |
| 4 | 1 PR | Inventory network-infra più metriche WG/k3s: localizzare guasto | A3 su fixture YAML/schema e query selettive; niente nuova discovery; CI. |
| 5 | 1 PR | Metriche consegna esistenti: separare alert da problemi di recapito | Failing/success counter fixture; non inviare ntfy né creare issue come test; CI. Fine MVP. |
| 6 | 1 PR | Max pain già v1: beneficio immediato a costo minimo dopo MVP | Import artifact readonly e fixture compatibilità maxpain; assenza/stale espliciti; CI. Nessun nuovo daemon. |
| 7 | 2–4 PR | Heatmap rektslug e chart nativo prezzo: contesto rischio per livello | Prima spike fixture A8 e payload, poi adapter, rendering e benchmark; CI e prova visiva su Mac. |
| 8 | 1–2 PR | Cockpit FastAPI/modelli esistenti: evidenze operative più ricche | Prima dimostrare endpoint/snapshot vivo; poi mapping carte A1/A2 senza copiare frontend; CI. |
| 9 | 1 PR di verifica, poi stima | GEX desktop esistente; pressione opzioni dopo: evitare doppio calcolo | Confronto metodologia/fonte su fixture; nessun nuovo algoritmo finché valore e differenze non sono chiariti. |

Non fare ora: nuova SPA/backend, grafi topologici interattivi, archivio notifiche,
comandi di gestione, nuovi agenti di discovery, nuovo calcolo GEX, deploy quality map
mascherato da lavoro UI. Costano più dell'accesso ai segnali già disponibili e ampliano
permessi/operatività senza rendere più utile l'MVP. Max pain precede heatmap per costo;
heatmap precede nuove analitiche opzioni per valore esplicitamente richiesto dall'operatore.

## Prove osservate e controprove, 2026-11-08

Sole richieste GET e ispezione unità, nessuna lettura segreti, modifica host o notifica:
Prometheus WG `10.0.0.7:30090` ready 200 (0,218 s); query `up` 50 serie (0,242 s),
`node_systemd_unit_state{name=~"nautilus-shadow.*"}` 40 (0,223 s), handshake WG 12 (0,220 s),
notifications_failed 45 (0,341 s). Query timestamp exporter shadow 1 serie (0,246 s),
`kube_node_info` 4 (0,221 s), `kube_pod_info` 46 (0,354 s).
Queste misure singole provano raggiungibilità, non p95, correttezza o SLA di produzione.
Rektslug locale `127.0.0.1:8002/health` 200 (0,013 s), OpenAPI senza sicurezza globale;
non estendere questa osservazione a tutte le installazioni/route.
SSH WG tk-nt-1: `nautilus-shadow.service`, `nautilus-shadow-nt.service` e producer shadow
attivi; timer ledger exporter attivo ogni minuto, relativo oneshot inattivo come previsto.
Controprove: dash-core `127.0.0.1:8790/v1/sheets` rifiuta connessione su questo host
(non prova un guasto del Mac); FastAPI `10.0.0.11:8765/api/status` non raggiunto entro 5 s.
Una query con nome metrica non esistente restituisce zero serie: non va trasformata in ok.
Heatmap completa, Mac, consegna ntfy e artifact qualità non sono stati verificati live;
sono dipendenze delle rispettive fasi, non funzionalità dichiarate già disponibili.

## Riferimenti riproducibili

Revisioni ispezionate; i percorsi seguenti sono relativi al rispettivo repository:

- [flowdepth develop 5867388](https://github.com/gptcompany/flowdepth/tree/5867388606ab07fc8d423c158d1a7fdd91dd88df):
  `dash-core/{README.md,src/model.rs,src/lib.rs,src/config.rs,src/sources/}`,
  `data/src/sheet.rs`, `src/widget/sheet.rs`, `src/{chart,connector}/gex.rs`,
  `dash-core/deploy/render-plist.py`, `.github/workflows/develop-ci.yml`.
- [network-infra 5b8cfd0](https://github.com/gptcompany/network-infra/tree/5b8cfd0920d46f680489be6f66148d24ddce5d36): `ansible/inventory/hosts.yml` e runbook WG.
- [monitoring-stack 9d0dc99](https://github.com/gptcompany/monitoring-stack/tree/9d0dc99092eeb50afb197725fd7a7d9bb79e414e): `prometheus/rules/`, `kubernetes/alertbus/`, `docs/runbook-alertbus.md`.
- [nautilus_dev d687e9f](https://github.com/gptcompany/nautilus_dev/tree/d687e9f9177df1726d67f19409e0395fc82328dc): `dashboard/{server.py,models.py}`, `specs/063-portfolio-operations-cockpit/`.
- [hyperliquid-node dda37b0](https://github.com/gptcompany/hyperliquid-node/tree/dda37b068241ff38b8bb775937258af50e9271e3): `sync/quality_map.py`, `specs/quality-map-review-350.md`.
- [rektslug ca8e3a7](https://github.com/gptcompany/rektslug/tree/ca8e3a72403e732d5ea4f9e26fc83e652e34eb4d): API heatmap-timeseries, prices/klines e modelli OpenAPI.
- [deribit-options-analysis 20715fa](https://github.com/gptcompany/deribit-options-analysis/tree/20715fa6a9d8f38551bef4afa4aa7a41b9be3e7a), [PR #10](https://github.com/gptcompany/deribit-options-analysis/pull/10): CLI flowdepth e test contratto maxpain.

## Domande riservate all'operatore

1. Confermare Mac/loopback come postazione iniziale, ruoli/host indispensabili e ordine MVP?
2. Per heatmap: quale simbolo, venue/quote e intervallo iniziale? È accettabile non mostrarla
   se le candele selezionate non corrispondono alla fonte della griglia?
3. Dove viene già pubblicato l'artifact max pain, con quale cadenza e budget stale?
4. Dopo MVP, Cockpit o heatmap ha priorità maggiore? Confermare GEX/pressione dopo il riuso
   di max pain e del grafico GEX esistente, senza nuova analitica in questa fase.
