# Projeto FedNow OSS — Requisitos de negócio e escopo técnico
*Documento vivo · v0.1 · julho/2026 · Joca + Claude*

## 1. Tese do projeto

A maioria das 1.500+ instituições no FedNow é **receive-only**: a barreira é o custo e
a complexidade de implementar a capacidade de **envio** sobre infraestrutura legada
(fonte: Fed + indústria, 2026 — mesma tese do Exhibit 15-B). O Fed mira ~8.000 das
10.000 instituições do país; o mercado de integração está aberto por anos.

**Missão:** reduzir o custo de integração de envio ao FedNow via tooling open-source
de referência (Apache-2.0), com simulador local e middleware de produção.

## 2. As três peças (monorepo)

| Peça | O que é | Entregável |
|---|---|---|
| `fednow-core` | Lib ISO 20022: parsing, validação (XSD + regras do Fed), construção e assinatura de mensagens | Primeiro milestone: parsear/validar um pacs.008 com CI verde |
| `fednow-sim` | Simulador do FedNow em Docker: aceita pacs.008, responde pacs.002, cenários de rejeição/timeout, RFP | Primeiro produto público (maior gap de DX do ecossistema — não há sandbox público de fácil acesso) |
| `fednow-gateway` | Middleware de envio: hexagonal, máquina de estados por pagamento, event sourcing, outbox, reconciliador (pacs.028) | Produto de produção; SDK Java/Python |

**Stack:** Rust no core/simulador; SDK Java e Python (mundo bancário US = Java; sem SDK Java, community bank não adota).

## 3. Requisitos técnicos derivados do FedNow real
*(fontes: Technical Overview and Planning Guide, Operating Procedures, Connectivity at a Glance)*

### Mensageria e transporte
- Troca de mensagens ISO 20022 é via **IBM MQ** (client middleware) + certificado de
  servidor do FedNow → o adapter sul do gateway precisa falar **MQ**, não só HTTP.
  O simulador deve expor interface compatível (MQ real ou emulação) + modo HTTP simples p/ dev.
- **mTLS obrigatório**: autenticação mútua com certificados emitidos pelo Fed.
- **Toda mensagem deve ser assinada criptograficamente**; o serviço valida assinatura
  e o vínculo entidade↔chave → `fednow-core` precisa de módulo de assinatura XML
  (XMLDSig) de primeira classe, não como afterthought.
- **APIs REST do FedNow** existem, mas não cobrem todos os tipos de mensagem —
  são complementares (consultas, connectivity check), acessadas via FedLine com
  certificado de API próprio. O gateway deve tratar API como canal auxiliar.
- Tipos de mensagem mínimos: pacs.008 (crédito), pacs.002 (status), pacs.028
  (status request), pain.013/014 (RFP), camt.056/029 (devolução), admi (ping/broadcast).

### Requisitos operacionais que o design deve espelhar
- **24x7x365 sem janela de manutenção** → deploy blue-green, zero-downtime.
- Participantes devem estar preparados p/ **alto volume** → benchmarks públicos no repo.
- **Fraude/risco em tempo real**: o Fed espera avaliação de risco por transação,
  24/7, com decisão de prosseguir/verificar → interface de hooks de risco
  (OFAC/velocity/scoring) plugável no gateway, pré-envio.
- Participation types: Customer Credit Transfer, Liquidity Management Transfer,
  Settlement Only → o gateway modela CCT primeiro; LMT depois.

### O caminho real até produção (contexto de quem usa o projeto)
- Onboarding do Fed em ~10 passos: Operating Circular 8 (OC 8, Apêndice A =
  Security Procedure Agreement), ferramenta digital de onboarding, conectividade
  (FedLine direta ou via service provider), **Customer Testing Program (CTP)** e
  **certificação de prontidão operacional** — obrigatórios p/ conexão direta e p/
  service providers (que devem certificar ANTES de onboardar terceiros).
- FI que conecta via service provider não precisa do CTP (mas teste é recomendado).
- **Fintech não participa diretamente**: precisa parceria com FI participante.
- Implicação de produto: o repo deve ter um **guia "do zero ao CTP"** — mapear cada
  test case de certificação a um cenário do simulador. O simulador vira ferramenta
  de preparação pro CTP (proposta de valor concreta e única).

## 4. Requisitos de confiança para adoção bancária
*(o que um banco/service provider avalia antes de adotar OSS — inclui guidance interagências de third-party risk)*

- Licença **Apache-2.0** no root (banco não adota copyleft).
- `SECURITY.md` com canal de disclosure + processo de CVE — desqualificante se ausente.
- CI público: build + testes + validação XSD a cada commit; releases **assinadas** com changelog.
- Suíte de **conformance** que qualquer implementação roda ("compatível com fednow-core vX").
- Documentação como produto: "FedNow Integration Handbook" (capítulo central:
  timeout não resolvido / reconciliação — onde mora 80% do sofrimento de produção).
- SBOM (software bill of materials) por release — bancos sob guidance de third-party
  risk pedem isso cedo.
- Sem telemetria/phone-home. Zero credencial no repo.
- Futuro (gatilho comercial): SOC 2 é da EMPRESA que presta serviço, não do repo —
  fica pra fase LLC.

## 5. Modelo de negócio e gatilhos

**Fase 1 — Repo (agora, do Brasil, sem empresa):**
- GitHub Sponsors (pessoa física recebe), consultoria remota faturada como PJ
  brasileiro (exportação de serviço — modelo que Joca já opera).
- Efeito-currículo: autor da lib de referência = primeira ligação do setor.

**Fase 2 — Primeiro interesse comercial (gatilho: e-mail de "queremos suporte pago"):**
- Abrir **LLC americana** (Delaware ou Wyoming; não residente pode abrir do Brasil;
  Mercury p/ conta; W-8/W-9 conforme o caso). Possuir empresa nos EUA é permitido
  a estrangeiro; trabalhar EM solo americano é que exige autorização — remoto do
  Brasil está fora da restrição.
- Produtos: contrato de suporte/SLA anual, integração paga, treinamento p/ CTP.

**Fase 3 — Escala (pós-green card):**
- Open-core (módulos enterprise: compliance dashboard, conectores de core bancário),
  versão hospedada (gateway-as-a-service p/ fintechs), patrocínio institucional
  (service providers / associações de community banks).

**Nota imigratória:** gerar renda com o projeto = executar a endeavor descrita na
petição. Contratos e patrocínios são evidência de primeira linha p/ fase consular
ou eventual refile. Consultar contador (tratado BR-EUA, timing da LLC) quando a
Fase 2 chegar.

## 6. GitHub — estrutura

- **Atenção:** o ToS do GitHub permite **uma conta pessoal gratuita por pessoa**.
  Em vez de segunda conta pessoal, criar uma **organização** (ex.: `fednow-oss`) —
  separa 100% dos projetos do dealership, permite mantenedores futuros, e o perfil
  pessoal aparece como founder. (Se ainda assim preferir conta separada, o caminho
  compatível é conta de trabalho/máquina paga ou org — org é o padrão da indústria.)
- Monorepo: `core/`, `simulator/`, `gateway/`, `docs/`, `conformance/`
  (Cargo workspaces).
- Público desde o primeiro commit, README honesto ("early development").
- Branch protection na main + PR obrigatório; GitHub Actions; Discussions ativado;
  `CONTRIBUTING.md`; issues como backlog do trabalho Joca+Claude (Claude Code
  apontado pro repo).
- Separação total: nada do processo imigratório dentro do repo.

## 7. Roadmap resumido

1. Org + monorepo + CI esqueleto.
2. `fednow-core`: pacs.008 parse/validate (XSD) + testes → **CI verde = milestone 1**.
3. pacs.002 + assinatura XMLDSig.
4. `fednow-sim` v0: recebe pacs.008, responde pacs.002 (aceite/rejeição configurável) em Docker.
5. Cenários de timeout + pacs.028 no simulador.
6. Handbook cap. 1 (fluxo de crédito) e cap. 2 (reconciliação de timeout).
7. `fednow-gateway` v0: API norte + máquina de estados + outbox + adapter → simulador.
8. Guia "do zero ao CTP" mapeando test cases de certificação a cenários do sim.

## 8. Fontes primárias (guardar)

- FedNow Readiness Guide (v. 25/02/2026) · Operating Procedures (Fed) ·
  Technical Overview and Planning Guide · Connectivity at a Glance ·
  Onboarding em 10 passos (frbservices.org) · Lista de participantes e
  service providers certificados (frbservices.org, atualizada mensalmente).
