# CLAUDE.md — Projeto FedNow OSS

## O que é este projeto
Tooling open-source (Apache-2.0) para reduzir o custo de integração de ENVIO ao
FedNow. Tese: a maioria das 1.500+ instituições na rede é receive-only porque
integrar envio é caro/complexo (fonte: Fed, 2026). Mercado-alvo: community banks,
credit unions e service providers dos EUA.

Contexto do autor: Joca — engenheiro de pagamentos BR, experiência Pix em produção
(varejo/POS), ISO 20022, idempotência.

## Monorepo — três peças (ordem de construção)
1. **fednow-core** — lib ISO 20022: parsing, validação (XSD + regras do Fed),
   construção e assinatura (XMLDSig) de mensagens.
   Tipos: pacs.008, pacs.002, pacs.028, pain.013, pain.014, camt.056, camt.029, admi.
2. **fednow-sim** — simulador FedNow em Docker: aceita pacs.008, responde pacs.002,
   cenários configuráveis (aceite, rejeição, timeout, participante receive-only),
   RFP (pain.013/014). PRIMEIRO PRODUTO PÚBLICO. Valor único: preparação pro
   Customer Testing Program (CTP) do Fed — mapear test cases de certificação a
   cenários do simulador.
3. **fednow-gateway** — middleware de envio, arquitetura hexagonal:
   - Porta norte: REST/gRPC, chave de idempotência OBRIGATÓRIA na criação.
   - Núcleo: máquina de estados por pagamento
     (CREATED→VALIDATED→SUBMITTED→ACK_PENDING→SETTLED|REJECTED|TIMEOUT_UNRESOLVED),
     persistida com event sourcing (eventos imutáveis).
   - Outbox pattern: gravação de estado + publicação atômicas (exactly-once efetivo).
   - Reconciliador: resolve ACK_PENDING além do timeout via pacs.028; NUNCA reenvio cego.
   - Porta sul: adapter trocável — FedNow real fala IBM MQ + mTLS + mensagens
     assinadas (certificados do Fed); em dev aponta pro fednow-sim.
   - Hooks de risco/fraude plugáveis pré-envio (OFAC, velocity). Ledger interno
     de dupla entrada p/ liquidez. OpenTelemetry.

## Stack e convenções
- Rust no core e simulador (Cargo workspaces: core/, simulator/, gateway/, docs/, conformance/).
- SDK Java e Python planejados p/ adoção bancária (mundo bancário US = Java).
- Design 24x7x365: zero-downtime, deploy blue-green. Sem telemetria/phone-home.
- Licença Apache-2.0. Releases assinadas + changelog. SBOM por release.
- SECURITY.md com canal de disclosure é obrigatório (bancos desqualificam sem).
- CI (GitHub Actions): build + testes + validação XSD em todo commit. Main protegida, PR obrigatório.
- Docs como produto: "FedNow Integration Handbook" em docs/ — capítulo central:
  reconciliação de timeout (o caso difícil de produção).
- Commits e código em inglês; discussão com Joca pode ser em PT-BR.

## Milestone atual
**v0.1.0 LANÇADA (jul/2026)** — loop completo de envio ponta a ponta:
core (5 mensagens nos perfis Release 1 reais, calibradas contra 81 samples
oficiais + builders), fednow-sim (6 cenários CTP + reconciliação pacs.028),
fednow-gateway (REST idempotente, event sourcing em SQLite, outbox real,
reconciliador de fundo), fednow-conformance (corpus de vetores + runner de
cenários), handbook (caps. timeout e zero-ao-CTP), release com SBOM.
**M4 (próximo): modo MQ (sim + adapter do gateway) e camt.056/029.**
Pendências externas: assinatura (issue #14 — Technical Specifications);
exports das guidelines de returns p/ calibrar pacs.004.

Repo: https://github.com/joaoabuenosi/fednow-oss (conta pessoal; org fica p/
quando houver mantenedores externos). Main protegida: PR + CI verde obrigatórios.

## Referências
- docs/requisitos.md — requisitos técnicos e de produto (fontes: FedNow
  Readiness Guide 02/2026, Operating Procedures, Technical Overview, Connectivity at a Glance).
- Notas de negócio/estratégia ficam FORA do repo (.private/ é gitignored).
- Sem credenciais no repo, nunca. Fixtures usam dados fictícios.
