# Porecatu

[![docs](https://github.com/porecatu/terminal/actions/workflows/docs.yml/badge.svg)](https://github.com/porecatu/terminal/actions/workflows/docs.yml)
[![ci](https://github.com/porecatu/terminal/actions/workflows/ci.yml/badge.svg)](https://github.com/porecatu/terminal/actions/workflows/ci.yml)
[![licença: GPL-3.0-or-later](https://img.shields.io/badge/licen%C3%A7a-GPL--3.0--or--later-blue)](LICENSE)

Um terminal para quem trabalha com **muitos terminais abertos ao mesmo tempo**.

Quem passa o dia na linha de comando conhece a cena: dez abas chamadas "bash", nenhuma pista de qual é qual, e, ao fechar a janela, todo o contexto de trabalho vai embora. O Porecatu foi feito para resolver isso: organizar, nomear, colorir e lembrar.

Versão atual: **0.9.2**. Funciona em Windows, Linux e macOS.

## O que ele faz

**Abas e grupos**
- Agrupe abas por assunto, dê um nome e uma cor a cada grupo, e recolha o grupo quando não precisar dele. Os programas continuam rodando por trás.
- Arraste abas entre grupos, mova um grupo inteiro e selecione várias abas de uma vez.
- Divida uma aba em vários terminais, lado a lado ou um embaixo do outro.

**Ele lembra do seu trabalho**
- Ao reabrir, as abas, os grupos, as janelas e as pastas voltam como estavam.
- Salve uma janela inteira com um nome e reabra quando quiser.
- Um arquivo `.porecatu` na pasta de um projeto pode dizer o que rodar quando a aba voltar.

**Do seu jeito**
- Cores, fonte, cursor, temas (claros e escuros), atalhos de teclado e imagem de fundo, tanto no terminal quanto na janela.
- Uma tela de configurações com tudo isso, ou, se preferir, um único arquivo de texto (`porecatu.toml`). Mudou, aplicou: sem reiniciar.
- Interface em português ou inglês.

**No dia a dia**
- Barra de status com a pasta atual, o shell e a branch do Git, e um aviso quando há commits novos no repositório remoto.
- Busca no histórico do terminal, links clicáveis, copiar e colar, mouse funcionando dentro de programas como `htop`.
- Mais de uma janela, tela cheia e suporte a leitores de tela.

Rápido por usar a placa de vídeo para desenhar, e sem gastar processador quando nada está acontecendo.

## Como instalar

Baixe o instalador da sua plataforma na [página de releases](https://github.com/porecatu/terminal/releases). O [guia do usuário](docs/guia-do-usuario.md) explica a instalação, a configuração e os atalhos, passo a passo.

Para rodar a partir do código-fonte (precisa do [Rust](https://rustup.rs)):

```bash
git clone https://github.com/porecatu/terminal.git
cd terminal
cargo run
```

## Estado do projeto

Todas as funções planejadas para a primeira versão estão prontas, e outras vieram depois, como painéis divididos, sessões nomeadas e a tela de configurações.

O Windows é a plataforma mais testada no uso real. Linux e macOS passam pelos testes automáticos, mas têm menos horas de uso. O que ainda falta conferir em cada fase está registrado no [roadmap](docs/roadmap.md).

## Para saber mais

- [Guia do usuário](docs/guia-do-usuario.md): instalação, configuração, atalhos
- [Configuração de exemplo](docs/config/porecatu.example.toml): todas as opções, comentadas
- [Visão do produto](docs/prd/prd-000-visao-de-produto.md): por que o Porecatu existe
- [Roadmap](docs/roadmap.md): o que foi feito e o que vem a seguir

Para quem quer contribuir: [CONTRIBUTING.md](CONTRIBUTING.md), [arquitetura](docs/arquitetura.md) e as [decisões técnicas](docs/adr/).

## O nome

Porecatu é uma cidade do norte do Paraná. O nome vem do tupi e significa **"salto bonito"**.

## Licença

**GPL-3.0-or-later.** Texto completo em [LICENSE](LICENSE). Copyright © 2026 Leonardo Otaviano Pedrozo.

O programa embute duas fontes: a [Iosevka](https://typeof.net/Iosevka/) (SIL Open Font License 1.1, [texto](assets/fonts/LICENSE-OFL-iosevka.txt)) e os ícones da [Lucide](https://lucide.dev) (licença ISC, [texto](assets/fonts/LICENSE-ISC-lucide.txt)).
