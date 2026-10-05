// SPDX-License-Identifier: GPL-3.0-or-later

//! Imagem de fundo do terminal: chave, estado e carga (PRD-017, ADR-0061
//! §1, §7 e §8).
//!
//! **Nada é desenhado aqui** (etapas 2 e 3 de 6). O módulo decide *quando* ler
//! uma imagem e *o que* fazer com o resultado; a pintura (etapa 4) vem depois.
//! A thread entrega os bytes RGBA8 e a cadeia de mips (`Decoded`); o `App` os
//! sobe para uma textura do `GpuContext` (`create_image`) e o estado passa a
//! guardar só o `ImageId` (`Ready`), com os bytes descartados.
//!
//! O desenho segue o das consultas ao Git (`git.rs`, ADR-0052): o estado é do
//! **processo** (`App`), nunca da janela; a carga roda numa thread de vida
//! curta e detached; e o que ela devolve é **dado chaveado**, nunca um comando
//! para mostrar algo -- é isso que faz a corrida "resultado de X chega com a
//! config já em Y" desaparecer por construção.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Seek};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::SystemTime;

use image::imageops::{self, FilterType};
use image::{ImageError, ImageFormat, ImageReader, Limits, RgbaImage};
use porecatu_config::BackgroundImageMode;
use porecatu_locale::Catalog;
use porecatu_render::{ImageId, Rect};

use crate::messages::msg;

/// Onde e como a textura cobre o quadro de um painel (ADR-0061 §6): os três
/// campos da `Primitive::Image` que dependem do modo. Tudo lógico, `uv` em
/// coordenadas de textura.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Placement {
    pub(crate) rect: Rect,
    pub(crate) uv: Rect,
    pub(crate) repeat: bool,
}

/// Geometria dos três modos, sem GPU (ADR-0061 §6, RF-17.5 e RF-17.6).
///
/// **Tamanho natural** é um pixel da imagem por pixel físico: `image_px /
/// scale` em lógico. A conta é feita em pixels **físicos**, depois de arredondar
/// as bordas do quadro ao pixel como `quad.rs` faz com todo retângulo
/// (`snap_rect_to_physical_pixels`: os dois cantos, não a largura isolada) --
/// assim a origem do `tile` e do `center` cai num pixel inteiro, cada texel
/// cai num pixel e a imagem sai nítida em tamanho natural.
///
/// - `stretch`: o quadro inteiro, `uv` `(0,0)-(1,1)`, sem repetir -- distorce.
/// - `tile`: o quadro inteiro, `uv` de `0` a `quadro / natural`, repetindo, a
///   partir do canto superior esquerdo.
/// - `center`: o tamanho natural, centrado no quadro e **intersectado** com ele
///   (maior que o quadro, é cortada nas bordas); `uv` é a fração da imagem
///   que cai dentro. Cada eixo é independente: maior num e menor no outro dá
///   um retângulo cortado de um lado e solto do outro.
///
/// Imagem de lado zero ou escala inválida cai no `stretch` (não há tamanho
/// natural a respeitar).
pub(crate) fn placement(
    mode: BackgroundImageMode,
    frame: Rect,
    image_px: (u32, u32),
    scale: f32,
) -> Placement {
    let whole = Rect {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    };
    let stretch = Placement {
        rect: frame,
        uv: whole,
        repeat: false,
    };
    let (image_w, image_h) = (image_px.0 as f32, image_px.1 as f32);
    if mode == BackgroundImageMode::Stretch
        || image_px.0 == 0
        || image_px.1 == 0
        || !(scale.is_finite() && scale > 0.0)
    {
        return stretch;
    }
    // O quadro em pixels físicos inteiros, como o `quad.rs` o vai arredondar.
    let (left, right) = (
        (frame.x * scale).round(),
        ((frame.x + frame.width) * scale).round(),
    );
    let (top, bottom) = (
        (frame.y * scale).round(),
        ((frame.y + frame.height) * scale).round(),
    );
    let (frame_w, frame_h) = (right - left, bottom - top);
    match mode {
        BackgroundImageMode::Stretch => stretch,
        BackgroundImageMode::Tile => Placement {
            rect: frame,
            uv: Rect {
                x: 0.0,
                y: 0.0,
                width: frame_w / image_w,
                height: frame_h / image_h,
            },
            repeat: true,
        },
        BackgroundImageMode::Center => {
            // Origem da imagem inteira, em pixel físico inteiro (pode ser
            // negativa: imagem maior que o quadro).
            let origin_x = left + ((frame_w - image_w) / 2.0).round();
            let origin_y = top + ((frame_h - image_h) / 2.0).round();
            let visible_left = origin_x.max(left);
            let visible_right = (origin_x + image_w).min(right);
            let visible_top = origin_y.max(top);
            let visible_bottom = (origin_y + image_h).min(bottom);
            Placement {
                rect: Rect {
                    x: visible_left / scale,
                    y: visible_top / scale,
                    width: (visible_right - visible_left).max(0.0) / scale,
                    height: (visible_bottom - visible_top).max(0.0) / scale,
                },
                uv: Rect {
                    x: (visible_left - origin_x) / image_w,
                    y: (visible_top - origin_y) / image_h,
                    width: (visible_right - visible_left).max(0.0) / image_w,
                    height: (visible_bottom - visible_top).max(0.0) / image_h,
                },
                repeat: false,
            }
        }
    }
}

/// O caminho resolvido de `raw` (o texto do campo da tela de configurações,
/// RF-17.3) **se** ele não existe no disco (RF-17.21); `None` com o campo
/// vazio ou com o arquivo presente. A tela chama isto a cada edição do campo:
/// é só um `exists`, sem decodificar -- formato e corrupção só saem no aviso
/// da recarga (RF-17.14). `config_path` é o arquivo de configuração em uso, base
/// de um caminho relativo.
pub(crate) fn missing_file(config_path: Option<&Path>, raw: &str) -> Option<PathBuf> {
    let resolved = porecatu_config::resolve_background_image_path(config_path, raw)?;
    (!resolved.exists()).then_some(resolved)
}

/// Maior lado de textura que o `Device` aceita com o `DeviceDescriptor::
/// default()` que o projeto pede (`max_texture_dimension_2d`). Só vale
/// enquanto não há `GpuContext` (a carga do arranque começa antes da primeira
/// janela, RF-17.17): com ele, `App` passa `GpuContext::
/// max_texture_dimension_2d()`.
pub(crate) const DEFAULT_MAX_TEXTURE_DIMENSION: u32 = 8192;

/// Por que a imagem não foi carregada (ADR-0061 §8). Sempre viaja ao lado do
/// caminho resolvido (`BackgroundImageKey::path`, `BackgroundImageFailure`):
/// a frase diz **qual** arquivo, e vem do catálogo, nunca do `Display` do
/// `image` (ADR-0056).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BackgroundImageError {
    /// O arquivo não existe no caminho resolvido.
    NotFound,
    /// Existe, mas a leitura falhou (permissão, é um diretório...).
    Unreadable(io::ErrorKind),
    /// Formato fora de PNG e JPEG -- ou conteúdo que nenhum formato conhecido
    /// reconhece. O formato vem do conteúdo, não da extensão (RF-17.2).
    UnsupportedFormat,
    /// Formato reconhecido, mas o arquivo está corrompido ou truncado.
    Malformed,
    /// Estourou o limite de memória que protege contra o arquivo pequeno que
    /// diz ter 100 000 x 100 000 pixels (RF-17.15). Imagem apenas maior que a
    /// textura da placa **não** cai aqui: é reduzida, sem aviso.
    TooLarge,
}

/// Erro mais o arquivo a que ele se refere: o que vira aviso (RF-17.14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BackgroundImageFailure {
    pub(crate) path: PathBuf,
    pub(crate) error: BackgroundImageError,
}

impl BackgroundImageFailure {
    /// Título e corpo do aviso, pelo catálogo (ADR-0056). Para o
    /// `unreadable` a frase não repete o `ErrorKind`: o `Display` dele é
    /// prosa em inglês da biblioteca padrão.
    pub(crate) fn notice_text(&self, catalog: &Catalog) -> (String, String) {
        let path = self.path.display();
        let body = match self.error {
            BackgroundImageError::NotFound => {
                msg::notice::background_image::not_found(catalog, path)
            }
            BackgroundImageError::Unreadable(_) => {
                msg::notice::background_image::unreadable(catalog, path)
            }
            BackgroundImageError::UnsupportedFormat => {
                msg::notice::background_image::unsupported(catalog, path)
            }
            BackgroundImageError::Malformed => {
                msg::notice::background_image::malformed(catalog, path)
            }
            BackgroundImageError::TooLarge => {
                msg::notice::background_image::too_large(catalog, path)
            }
        };
        (msg::notice::background_image::title(catalog), body)
    }
}

/// Identidade de uma carga: caminho resolvido, `mtime` e tamanho do arquivo
/// (ADR-0061 §7). Igual à atual, nada acontece -- é o caso de mudar só `mode`
/// ou `opacity`. Diferente, é uma carga nova, e um resultado com a chave
/// velha é descartado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BackgroundImageKey {
    pub(crate) path: PathBuf,
    /// `None` quando o arquivo não deu `metadata` ou o sistema de arquivos
    /// não guarda `mtime`.
    pub(crate) mtime: Option<SystemTime>,
    pub(crate) len: u64,
}

impl BackgroundImageKey {
    /// Um `metadata`, barato, na main thread (a mesma ordem de custo do `stat`
    /// do `.git/HEAD`, ADR-0049). Arquivo que não dá `metadata` ainda tem
    /// chave -- `mtime: None`, `len: 0` --, para que a falha seja
    /// deduplicada como qualquer outra: o mesmo caminho, ainda ausente, não
    /// avisa de novo a cada recarga; quando o arquivo aparece, a chave muda e
    /// a carga acontece.
    pub(crate) fn probe(path: &Path) -> (Self, Option<BackgroundImageError>) {
        match std::fs::metadata(path) {
            Ok(meta) => (
                Self {
                    path: path.to_path_buf(),
                    mtime: meta.modified().ok(),
                    len: meta.len(),
                },
                None,
            ),
            Err(err) => (
                Self {
                    path: path.to_path_buf(),
                    mtime: None,
                    len: 0,
                },
                Some(io_error(&err)),
            ),
        }
    }
}

/// Um nível da cadeia de mips: RGBA8, alfa reto (o shader premultiplica,
/// ADR-0061 §3), `width * height * 4` bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MipLevel {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rgba: Vec<u8>,
}

/// O que a thread de carga entrega: a imagem já reduzida a `max_dim`, com a
/// cadeia de mips inteira (o nível 0 é a própria imagem).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DecodedImage {
    /// Tamanho do nível 0, **depois** da redução ao limite da textura.
    pub(crate) size: (u32, u32),
    pub(crate) levels: Vec<MipLevel>,
}

/// A imagem já na GPU: o identificador do registro de `porecatu-render`
/// (`GpuContext::create_image`) e o tamanho do nível 0. Os bytes ficaram para
/// trás -- a textura é do processo, e toda janela desenha com ela (RF-17.18).
///
/// Genérico no handle (`H`, o `ImageId` por padrão) só para que o store seja
/// testável sem GPU: `ImageId` é opaco e só o registro cria um.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BackgroundTexture<H = ImageId> {
    pub(crate) id: H,
    pub(crate) size: (u32, u32),
}

/// Estado da imagem da chave atual (ADR-0061 §7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BackgroundImageState<H = ImageId> {
    Loading,
    /// A thread entregou os bytes e a textura ainda não foi criada -- por não
    /// haver `GpuContext` ainda (a carga acabou antes da primeira janela). É
    /// um intervalo, não um estado de repouso: `upload` o resolve.
    Decoded(DecodedImage),
    Ready(BackgroundTexture<H>),
    Failed(BackgroundImageError),
}

/// O que chega da thread de carga: o resultado **com a chave que o pediu**.
#[derive(Debug)]
pub(crate) struct BackgroundImageResult {
    pub(crate) key: BackgroundImageKey,
    pub(crate) outcome: Result<DecodedImage, BackgroundImageError>,
}

/// O que `BackgroundImageStore::sync` pede a quem chama.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SyncOutcome {
    /// Mesma chave (ou continua sem imagem): nada a fazer.
    Unchanged,
    /// `path` ficou vazio: a imagem, e a anterior, saíram.
    Cleared,
    /// Chave nova: abrir a thread de carga com ela.
    Load(BackgroundImageKey),
    /// Chave nova que já falhou no `metadata`, sem thread nenhuma: avisar.
    Failed(BackgroundImageFailure),
}

/// O que `BackgroundImageStore::apply` conta de um resultado que chegou.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ApplyOutcome {
    /// A chave do resultado não é mais a atual: descartado, nada mudou.
    Discarded,
    /// Os bytes da chave atual chegaram: falta criar a textura (`upload`).
    Decoded,
    /// A carga da chave atual falhou: avisar, uma vez.
    Failed(BackgroundImageFailure),
}

/// O que `BackgroundImageStore::upload` conta.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum UploadOutcome {
    /// Nada esperando textura (estado que não é `Decoded`).
    Nothing,
    /// A textura foi criada: a imagem da chave atual está pronta.
    Ready,
    /// A textura não pôde ser criada (maior que o limite da placa): avisar.
    Failed(BackgroundImageFailure),
}

/// Um resultado só vale se a chave dele **ainda é a atual** (ADR-0061 §7).
/// Função pura: é a que faz a corrida de "duas recargas seguidas" sumir.
pub(crate) fn result_is_current(
    current: Option<&BackgroundImageKey>,
    result: &BackgroundImageKey,
) -> bool {
    current == Some(result)
}

/// A imagem de fundo do **processo** (`App`), nunca da janela: uma decodificação
/// por processo, qualquer que seja o número de janelas e de painéis (RF-17.18).
///
/// Puro: não toca o disco, a GPU nem abre thread -- quem faz isso é o `App`, a
/// partir do que `sync`, `apply` e `upload` devolvem. É o que o torna testável
/// sem janela. A GPU entra por um fecho (`upload`) e pela lista de texturas a
/// soltar (`drain_released`).
#[derive(Debug)]
pub(crate) struct BackgroundImageStore<H = ImageId> {
    current: Option<(BackgroundImageKey, BackgroundImageState<H>)>,
    /// A última textura `Ready`, mantida **enquanto a chave nova carrega**:
    /// trocar de imagem não pisca sem imagem no meio (ADR-0061 §7). Sai
    /// quando a nova fica pronta, falha, ou `path` fica vazio.
    previous: Option<BackgroundTexture<H>>,
    /// Texturas que o store largou e que a GPU ainda guarda: o `App` chama
    /// `remove_image` para cada uma (`drain_released`).
    released: Vec<H>,
}

// À mão: o `derive` exigiria `H: Default`, e `ImageId` não é.
impl<H> Default for BackgroundImageStore<H> {
    fn default() -> Self {
        Self {
            current: None,
            previous: None,
            released: Vec::new(),
        }
    }
}

impl<H: Copy> BackgroundImageStore<H> {
    /// Chave da carga atual, `None` sem imagem configurada.
    pub(crate) fn key(&self) -> Option<&BackgroundImageKey> {
        self.current.as_ref().map(|(key, _)| key)
    }

    /// Estado da chave atual.
    #[allow(dead_code)] // Lido pela pintura (etapa 4) e pelos testes.
    pub(crate) fn state(&self) -> Option<&BackgroundImageState<H>> {
        self.current.as_ref().map(|(_, state)| state)
    }

    /// A textura que deve ser desenhada agora: a da chave atual, se pronta;
    /// senão a anterior, enquanto a nova carrega; senão nenhuma.
    pub(crate) fn displayed(&self) -> Option<BackgroundTexture<H>> {
        match self.state()? {
            BackgroundImageState::Ready(texture) => Some(*texture),
            BackgroundImageState::Loading | BackgroundImageState::Decoded(_) => self.previous,
            BackgroundImageState::Failed(_) => None,
        }
    }

    /// Texturas que o store largou desde a última chamada, para o `App`
    /// soltar na GPU (`remove_image`). Sem `GpuContext` (nunca houve textura)
    /// a lista fica vazia.
    pub(crate) fn drain_released(&mut self) -> Vec<H> {
        std::mem::take(&mut self.released)
    }

    fn release_previous(&mut self) {
        if let Some(texture) = self.previous.take() {
            self.released.push(texture.id);
        }
    }

    /// Reconcilia o estado com o que a config pede agora: chamado a cada
    /// aplicação de config (arranque e cada recarga). `wanted` é a chave do
    /// caminho resolvido (`None` com `path` vazio), com o erro do `metadata`
    /// quando ele falhou (`BackgroundImageKey::probe`).
    pub(crate) fn sync(
        &mut self,
        wanted: Option<(BackgroundImageKey, Option<BackgroundImageError>)>,
    ) -> SyncOutcome {
        let Some((key, probe_error)) = wanted else {
            self.release_previous();
            return match self.current.take() {
                Some((_, state)) => {
                    if let BackgroundImageState::Ready(texture) = state {
                        self.released.push(texture.id);
                    }
                    SyncOutcome::Cleared
                }
                None => SyncOutcome::Unchanged,
            };
        };
        if self.key() == Some(&key) {
            return SyncOutcome::Unchanged;
        }
        // Chave nova. A textura pronta da chave que sai passa a ser a
        // "anterior", mantida até a nova ficar pronta; `previous` que já
        // existia (carga em andamento que foi trocada) continua valendo, e a
        // que sobraria é solta (não acontece: `previous` só existe enquanto a
        // atual não está `Ready`).
        if let Some((_, BackgroundImageState::Ready(texture))) = self.current.take() {
            self.release_previous();
            self.previous = Some(texture);
        }
        if let Some(error) = probe_error {
            self.release_previous();
            self.current = Some((key.clone(), BackgroundImageState::Failed(error)));
            return SyncOutcome::Failed(BackgroundImageFailure {
                path: key.path,
                error,
            });
        }
        self.current = Some((key.clone(), BackgroundImageState::Loading));
        SyncOutcome::Load(key)
    }

    /// Aplica o resultado de uma carga: descarta se a chave dele não é mais a
    /// atual; senão guarda, e uma falha devolve o que avisar. Uma chave só
    /// sai de `Loading` uma vez, então o aviso sai uma vez por arquivo e por
    /// problema (RF-17.14).
    pub(crate) fn apply(&mut self, result: BackgroundImageResult) -> ApplyOutcome {
        if !result_is_current(self.key(), &result.key) {
            return ApplyOutcome::Discarded;
        }
        let Some((_, state)) = &mut self.current else {
            return ApplyOutcome::Discarded;
        };
        if !matches!(state, BackgroundImageState::Loading) {
            return ApplyOutcome::Discarded;
        }
        match result.outcome {
            Ok(image) => {
                // A anterior fica até `upload` criar a textura nova.
                *state = BackgroundImageState::Decoded(image);
                ApplyOutcome::Decoded
            }
            Err(error) => {
                *state = BackgroundImageState::Failed(error);
                self.release_previous();
                ApplyOutcome::Failed(BackgroundImageFailure {
                    path: result.key.path,
                    error,
                })
            }
        }
    }

    /// Cria a textura da imagem decodificada, se há uma esperando: `create`
    /// recebe os bytes e devolve o `ImageId` do registro de `porecatu-render`.
    /// Os bytes saem do estado (só o `id` e o tamanho ficam) e a textura
    /// anterior é solta. Chamado logo depois de `apply` e, se ainda não havia
    /// `GpuContext`, quando ele passa a existir.
    pub(crate) fn upload(
        &mut self,
        create: impl FnOnce(&DecodedImage) -> Result<H, BackgroundImageError>,
    ) -> UploadOutcome {
        let Some((key, state)) = self.current.as_mut() else {
            return UploadOutcome::Nothing;
        };
        let BackgroundImageState::Decoded(image) = &*state else {
            return UploadOutcome::Nothing;
        };
        let size = image.size;
        let outcome = match create(image) {
            Ok(id) => {
                *state = BackgroundImageState::Ready(BackgroundTexture { id, size });
                UploadOutcome::Ready
            }
            Err(error) => {
                let failure = BackgroundImageFailure {
                    path: key.path.clone(),
                    error,
                };
                *state = BackgroundImageState::Failed(error);
                UploadOutcome::Failed(failure)
            }
        };
        self.release_previous();
        outcome
    }
}

/// Abre a thread de carga, de vida curta e detached (ADR-0061 §7): mesma
/// disciplina de `reload::watch` e de `git::spawn_query` -- sem `join`, o
/// processo inteiro sai junto dela. `on_result` roda **na thread da carga**,
/// nunca na main; quem chama passa um fecho que só manda o resultado pelo
/// `EventLoopProxy`.
///
/// Um pânico do decodificador derruba a thread, não o app (ADR-0061, riscos):
/// ele é pego aqui e vira `Malformed`, para que a chave não fique em `Loading`
/// para sempre.
pub(crate) fn spawn_load(
    key: BackgroundImageKey,
    max_dim: u32,
    on_result: impl FnOnce(BackgroundImageResult) + Send + 'static,
) {
    thread::spawn(move || {
        let outcome = panic::catch_unwind(AssertUnwindSafe(|| load_file(&key.path, max_dim)))
            .unwrap_or(Err(BackgroundImageError::Malformed));
        on_result(BackgroundImageResult { key, outcome });
    });
}

fn load_file(path: &Path, max_dim: u32) -> Result<DecodedImage, BackgroundImageError> {
    let file = File::open(path).map_err(|err| io_error(&err))?;
    decode(BufReader::new(file), max_dim)
}

/// Lê, reconhece o formato pelo conteúdo, converte para RGBA8, reduz até
/// `max_dim` mantendo a proporção e gera os mips. Separado de `load_file`
/// para os testes alimentarem bytes em memória.
pub(crate) fn decode<R: BufRead + Seek>(
    source: R,
    max_dim: u32,
) -> Result<DecodedImage, BackgroundImageError> {
    let mut reader = ImageReader::new(source)
        .with_guessed_format()
        .map_err(|err| io_error(&err))?;
    // RF-17.2: o formato é o do conteúdo. Sem as features padrão o `image` só
    // sabe decodificar estes dois, mas *reconhece* mais (um GIF sai como
    // `Gif`, não como lixo): reconhecer e recusar dá o mesmo erro, e a
    // checagem explícita não depende de qual decodificador está compilado.
    if !matches!(reader.format(), Some(ImageFormat::Png | ImageFormat::Jpeg)) {
        return Err(BackgroundImageError::UnsupportedFormat);
    }
    // RF-17.15: os limites padrão do crate, sem número nosso.
    reader.limits(Limits::default());
    let rgba = reader
        .decode()
        .map_err(|err| image_error(&err))?
        .into_rgba8();
    let base = reduce_to_fit(rgba, max_dim);
    let size = base.dimensions();
    Ok(DecodedImage {
        size,
        levels: build_mips(base),
    })
}

fn io_error(err: &io::Error) -> BackgroundImageError {
    match err.kind() {
        io::ErrorKind::NotFound => BackgroundImageError::NotFound,
        // Arquivo que acaba antes do que o cabeçalho prometeu.
        io::ErrorKind::UnexpectedEof => BackgroundImageError::Malformed,
        kind => BackgroundImageError::Unreadable(kind),
    }
}

fn image_error(err: &ImageError) -> BackgroundImageError {
    match err {
        ImageError::Limits(_) => BackgroundImageError::TooLarge,
        ImageError::Unsupported(_) => BackgroundImageError::UnsupportedFormat,
        ImageError::IoError(err) => io_error(err),
        _ => BackgroundImageError::Malformed,
    }
}

/// Tamanho que cabe em `max_dim` nos dois lados, mantendo a proporção; o
/// próprio tamanho se já cabe. Nunca zero, nunca acima de `max_dim`.
pub(crate) fn fit_dimensions(width: u32, height: u32, max_dim: u32) -> (u32, u32) {
    let max_dim = max_dim.max(1);
    let longest = width.max(height);
    if longest <= max_dim {
        return (width, height);
    }
    let scale = f64::from(max_dim) / f64::from(longest);
    let scaled = |side: u32| ((f64::from(side) * scale).round() as u32).clamp(1, max_dim);
    (scaled(width), scaled(height))
}

fn reduce_to_fit(image: RgbaImage, max_dim: u32) -> RgbaImage {
    let (width, height) = image.dimensions();
    let (new_width, new_height) = fit_dimensions(width, height, max_dim);
    if (new_width, new_height) == (width, height) {
        return image;
    }
    // Redimensionar RGBA de alfa reto franjaria nas bordas transparentes (a cor
    // de um texel de alfa 0 entra na média); com alfa variável, reduz-se em
    // espaço premultiplicado e desfaz-se depois.
    let has_alpha = image.pixels().any(|px| px[3] != 255);
    let mut image = image;
    if has_alpha {
        premultiply(&mut image);
    }
    let mut reduced = imageops::resize(&image, new_width, new_height, FilterType::Triangle);
    if has_alpha {
        unpremultiply(&mut reduced);
    }
    reduced
}

fn premultiply(image: &mut RgbaImage) {
    for px in image.pixels_mut() {
        let alpha = u32::from(px[3]);
        for channel in &mut px.0[..3] {
            *channel = ((u32::from(*channel) * alpha + 127) / 255) as u8;
        }
    }
}

fn unpremultiply(image: &mut RgbaImage) {
    for px in image.pixels_mut() {
        let alpha = u32::from(px[3]);
        for channel in &mut px.0[..3] {
            // Alfa 0 não tem cor a recuperar.
            *channel = (u32::from(*channel) * 255 + alpha / 2)
                .checked_div(alpha)
                .map_or(0, |value| value.min(255) as u8);
        }
    }
}

/// Cadeia de mips por redução sucessiva (ADR-0061 §3), do nível 0 (a imagem)
/// até 1x1: `floor(log2(max(w, h))) + 1` níveis. Sem ela, um JPEG de 6000 px
/// esticado num painel de 600 px cintila e serrilha.
fn build_mips(base: RgbaImage) -> Vec<MipLevel> {
    let (width, height) = base.dimensions();
    let mut levels = vec![MipLevel {
        width,
        height,
        rgba: base.into_raw(),
    }];
    while let Some(last) = levels.last()
        && (last.width > 1 || last.height > 1)
    {
        let next = next_level(last);
        levels.push(next);
    }
    levels
}

/// Metade do nível, por média 2x2 **ponderada pelo alfa** (equivale a
/// premultiplicar, tirar a média e desfazer): a cor de um texel transparente
/// não vaza para os vizinhos. Lado ímpar repete o último texel.
fn next_level(prev: &MipLevel) -> MipLevel {
    let width = (prev.width / 2).max(1);
    let height = (prev.height / 2).max(1);
    let texel = |x: u32, y: u32| -> [u32; 4] {
        let x = x.min(prev.width - 1) as usize;
        let y = y.min(prev.height - 1) as usize;
        let at = (y * prev.width as usize + x) * 4;
        let px = &prev.rgba[at..at + 4];
        [
            u32::from(px[0]),
            u32::from(px[1]),
            u32::from(px[2]),
            u32::from(px[3]),
        ]
    };
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        for x in 0..width {
            let samples = [
                texel(2 * x, 2 * y),
                texel(2 * x + 1, 2 * y),
                texel(2 * x, 2 * y + 1),
                texel(2 * x + 1, 2 * y + 1),
            ];
            let alpha_sum: u32 = samples.iter().map(|s| s[3]).sum();
            for channel in 0..3 {
                let weighted: u32 = samples.iter().map(|s| s[channel] * s[3]).sum();
                // Soma de alfa 0: quatro texels transparentes, sem cor.
                rgba.push(
                    (weighted + alpha_sum / 2)
                        .checked_div(alpha_sum)
                        .map_or(0, |value| value as u8),
                );
            }
            rgba.push(((alpha_sum + 2) / 4) as u8);
        }
    }
    MipLevel {
        width,
        height,
        rgba,
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::time::Duration;

    use image::{DynamicImage, Rgba};

    use super::*;

    fn gradient(width: u32, height: u32) -> RgbaImage {
        RgbaImage::from_fn(width, height, |x, y| {
            Rgba([
                (x * 255 / width.max(1)) as u8,
                (y * 255 / height.max(1)) as u8,
                90,
                255,
            ])
        })
    }

    fn encode(image: &RgbaImage, format: ImageFormat) -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        // O JPEG não tem alfa: o encoder recusa RGBA, então vai como RGB.
        let dynamic = match format {
            ImageFormat::Jpeg => {
                DynamicImage::ImageRgb8(DynamicImage::ImageRgba8(image.clone()).into_rgb8())
            }
            _ => DynamicImage::ImageRgba8(image.clone()),
        };
        dynamic.write_to(&mut bytes, format).expect("encode");
        bytes.into_inner()
    }

    fn decode_bytes(bytes: Vec<u8>, max_dim: u32) -> Result<DecodedImage, BackgroundImageError> {
        decode(Cursor::new(bytes), max_dim)
    }

    fn key(name: &str, len: u64) -> BackgroundImageKey {
        BackgroundImageKey {
            path: PathBuf::from(name),
            mtime: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(len)),
            len,
        }
    }

    fn tiny() -> DecodedImage {
        DecodedImage {
            size: (1, 1),
            levels: vec![MipLevel {
                width: 1,
                height: 1,
                rgba: vec![1, 2, 3, 255],
            }],
        }
    }

    fn loaded(key: &BackgroundImageKey) -> BackgroundImageResult {
        BackgroundImageResult {
            key: key.clone(),
            outcome: Ok(tiny()),
        }
    }

    // ---- decodificação -------------------------------------------------

    #[test]
    fn png_decodes_with_its_dimensions_and_pixels() {
        let source = gradient(16, 8);
        let decoded = decode_bytes(encode(&source, ImageFormat::Png), 8192).unwrap();
        assert_eq!(decoded.size, (16, 8));
        let base = &decoded.levels[0];
        assert_eq!((base.width, base.height), (16, 8));
        // PNG é sem perda: o nível 0 é o original, byte a byte.
        assert_eq!(base.rgba, source.into_raw());
    }

    #[test]
    fn jpeg_decodes_with_its_dimensions() {
        let decoded = decode_bytes(encode(&gradient(32, 24), ImageFormat::Jpeg), 8192).unwrap();
        assert_eq!(decoded.size, (32, 24));
        assert_eq!(decoded.levels[0].rgba.len(), 32 * 24 * 4);
        // JPEG não tem alfa: todo texel sai opaco.
        assert!(decoded.levels[0].rgba.chunks(4).all(|px| px[3] == 255));
    }

    #[test]
    fn png_alpha_is_preserved() {
        let mut source = gradient(4, 4);
        source.put_pixel(1, 1, Rgba([200, 10, 10, 64]));
        let decoded = decode_bytes(encode(&source, ImageFormat::Png), 8192).unwrap();
        let at = (4 + 1) * 4;
        assert_eq!(&decoded.levels[0].rgba[at..at + 4], &[200, 10, 10, 64]);
    }

    #[test]
    fn format_comes_from_the_content_not_the_name() {
        // `decode` nem recebe um nome: PNG por dentro é PNG, e JPEG também.
        assert!(decode_bytes(encode(&gradient(4, 4), ImageFormat::Png), 64).is_ok());
        assert!(decode_bytes(encode(&gradient(4, 4), ImageFormat::Jpeg), 64).is_ok());
    }

    #[test]
    fn larger_than_max_dim_is_reduced_keeping_the_aspect() {
        let decoded = decode_bytes(encode(&gradient(64, 32), ImageFormat::Png), 16).unwrap();
        assert_eq!(decoded.size, (16, 8));
        assert_eq!(decoded.levels[0].rgba.len(), 16 * 8 * 4);
    }

    #[test]
    fn smaller_than_max_dim_is_left_alone() {
        let decoded = decode_bytes(encode(&gradient(10, 6), ImageFormat::Png), 16).unwrap();
        assert_eq!(decoded.size, (10, 6));
    }

    #[test]
    fn reduction_never_gives_a_zero_side() {
        assert_eq!(fit_dimensions(1000, 1, 8), (8, 1));
        assert_eq!(fit_dimensions(1, 1000, 8), (1, 8));
        assert_eq!(fit_dimensions(8192, 8192, 8192), (8192, 8192));
        assert_eq!(fit_dimensions(16384, 8192, 8192), (8192, 4096));
        assert_eq!(fit_dimensions(5, 5, 0), (1, 1));
    }

    #[test]
    fn the_mip_chain_goes_down_to_one_by_one() {
        // floor(log2(max(w, h))) + 1 níveis.
        let cases = [
            (16, 16, 5),
            (16, 8, 5),
            (10, 6, 4),
            (1, 1, 1),
            (3, 1, 2),
            (64, 1, 7),
        ];
        for (width, height, count) in cases {
            let decoded =
                decode_bytes(encode(&gradient(width, height), ImageFormat::Png), 8192).unwrap();
            assert_eq!(decoded.levels.len(), count, "{width}x{height}");
            let last = decoded.levels.last().unwrap();
            assert_eq!((last.width, last.height), (1, 1), "{width}x{height}");
        }
    }

    #[test]
    fn every_level_is_half_the_previous_and_fully_sized() {
        let decoded = decode_bytes(encode(&gradient(40, 24), ImageFormat::Png), 8192).unwrap();
        for pair in decoded.levels.windows(2) {
            assert_eq!(pair[1].width, (pair[0].width / 2).max(1));
            assert_eq!(pair[1].height, (pair[0].height / 2).max(1));
        }
        for level in &decoded.levels {
            assert_eq!(level.rgba.len(), (level.width * level.height * 4) as usize);
        }
    }

    #[test]
    fn mip_of_a_solid_color_is_that_color() {
        let solid = RgbaImage::from_pixel(8, 8, Rgba([10, 120, 250, 255]));
        let decoded = decode_bytes(encode(&solid, ImageFormat::Png), 8192).unwrap();
        for level in &decoded.levels {
            assert!(level.rgba.chunks(4).all(|px| px == [10, 120, 250, 255]));
        }
    }

    #[test]
    fn transparent_texels_do_not_tint_the_mip() {
        // Metade opaca vermelha, metade transparente com cor verde: o mip tem
        // de sair vermelho (alfa 128), não amarelado pela cor do texel
        // invisível.
        let image = RgbaImage::from_fn(2, 1, |x, _| {
            if x == 0 {
                Rgba([255, 0, 0, 255])
            } else {
                Rgba([0, 255, 0, 0])
            }
        });
        let decoded = decode_bytes(encode(&image, ImageFormat::Png), 8192).unwrap();
        assert_eq!(decoded.levels.len(), 2);
        assert_eq!(decoded.levels[1].rgba, vec![255, 0, 0, 128]);
    }

    #[test]
    fn reduction_by_max_dim_does_not_tint_transparent_edges() {
        // Tudo transparente com cor verde, menos um texel vermelho opaco.
        let image = RgbaImage::from_fn(8, 8, |x, y| {
            if (x, y) == (0, 0) {
                Rgba([255, 0, 0, 255])
            } else {
                Rgba([0, 255, 0, 0])
            }
        });
        let decoded = decode_bytes(encode(&image, ImageFormat::Png), 4).unwrap();
        assert_eq!(decoded.size, (4, 4));
        let first = &decoded.levels[0].rgba[..4];
        assert!(first[3] > 0, "o texel vermelho sobrevive à redução");
        assert!(
            first[0] > 200 && first[1] < 30,
            "sem verde vazando: {first:?}"
        );
    }

    // ---- nota de "arquivo não encontrado" (RF-17.21), função pura ------

    #[test]
    fn missing_file_is_none_for_an_empty_path_or_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("porecatu.toml");
        std::fs::write(dir.path().join("fundo.png"), b"x").unwrap();
        assert_eq!(missing_file(Some(&config), ""), None);
        assert_eq!(missing_file(Some(&config), "  "), None);
        // Relativo, existente.
        assert_eq!(missing_file(Some(&config), "fundo.png"), None);
        // Absoluto, existente.
        let absolute = dir.path().join("fundo.png");
        assert_eq!(missing_file(None, absolute.to_str().unwrap()), None);
    }

    #[test]
    fn missing_file_gives_the_resolved_path_when_it_does_not_exist() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("porecatu.toml");
        // Relativo: resolvido contra a pasta do config, não contra onde o app
        // foi lançado.
        assert_eq!(
            missing_file(Some(&config), "imagens/sumiu.png"),
            Some(dir.path().join("imagens/sumiu.png"))
        );
        // Absoluto: como está.
        let absolute = dir.path().join("sumiu.jpg");
        assert_eq!(
            missing_file(None, absolute.to_str().unwrap()),
            Some(absolute.clone())
        );
        // O mesmo nome, relativo, existindo noutra pasta: continua faltando
        // (a regra é a pasta do config).
        let other = tempfile::tempdir().unwrap();
        std::fs::write(other.path().join("so-aqui.png"), b"x").unwrap();
        assert!(missing_file(Some(&config), "so-aqui.png").is_some());
    }

    #[test]
    fn missing_file_sees_a_directory_as_present() {
        // `exists` não distingue; a recarga é quem avisa que não é imagem.
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("porecatu.toml");
        std::fs::create_dir(dir.path().join("pasta")).unwrap();
        assert_eq!(missing_file(Some(&config), "pasta"), None);
    }

    // ---- placement (função pura, sem GPU) ------------------------------

    const SCALES: [f32; 4] = [1.0, 1.25, 1.5, 2.0];

    fn r(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    fn assert_rect_near(actual: Rect, expected: Rect, what: &str) {
        assert_rect_within(actual, expected, 1e-3, what);
    }

    /// A origem do `center` é arredondada ao pixel físico, então o desenho
    /// pode ficar até meio pixel do centro exato: `tolerance` é esse meio
    /// pixel (com folga) na unidade do campo comparado.
    fn assert_rect_within(actual: Rect, expected: Rect, tolerance: f32, what: &str) {
        let close = |a: f32, e: f32| (a - e).abs() <= tolerance;
        assert!(
            close(actual.x, expected.x)
                && close(actual.y, expected.y)
                && close(actual.width, expected.width)
                && close(actual.height, expected.height),
            "{what}: {actual:?} != {expected:?}"
        );
    }

    #[test]
    fn stretch_is_the_whole_frame_and_the_whole_texture_at_every_scale() {
        let frame = r(6.0, 52.0, 400.0, 300.0);
        for scale in SCALES {
            for image in [(10, 10), (4000, 3000)] {
                let p = placement(BackgroundImageMode::Stretch, frame, image, scale);
                assert_eq!(p.rect, frame, "escala {scale}");
                assert_eq!(p.uv, r(0.0, 0.0, 1.0, 1.0));
                assert!(!p.repeat);
            }
        }
    }

    #[test]
    fn tile_covers_the_frame_with_one_texel_per_physical_pixel() {
        // Quadro com bordas em pixel inteiro em toda escala testada.
        let frame = r(8.0, 40.0, 400.0, 200.0);
        for scale in SCALES {
            let p = placement(BackgroundImageMode::Tile, frame, (200, 100), scale);
            assert_eq!(p.rect, frame, "o `rect` é o quadro, escala {scale}");
            assert!(p.repeat);
            assert_eq!(
                (p.uv.x, p.uv.y),
                (0.0, 0.0),
                "ancorado no canto superior esquerdo"
            );
            // Um texel por pixel físico: o quadro tem `400 * scale` pixels, e a
            // imagem 200 -- `uv.width` repetições.
            assert!(
                (p.uv.width - 400.0 * scale / 200.0).abs() < 1e-4,
                "escala {scale}: {}",
                p.uv.width
            );
            assert!((p.uv.height - 200.0 * scale / 100.0).abs() < 1e-4);
        }
    }

    #[test]
    fn tile_with_a_frame_edge_between_pixels_uses_the_snapped_extent() {
        // Largura 100.3 a escala 1.0: o quad vai de x=10 a x=111 (os dois
        // cantos arredondados), 101 pixels -- o `uv` tem de contar esses 101,
        // não 100.3, ou o último texel sai esticado.
        let frame = r(10.4, 0.0, 100.3, 50.0);
        let p = placement(BackgroundImageMode::Tile, frame, (10, 10), 1.0);
        let left = (10.4_f32).round();
        let right = (10.4_f32 + 100.3).round();
        assert!((p.uv.width - (right - left) / 10.0).abs() < 1e-5);
    }

    #[test]
    fn center_with_a_smaller_image_is_natural_size_in_the_middle() {
        let frame = r(0.0, 0.0, 400.0, 200.0);
        for scale in SCALES {
            // 200x100 pixels físicos: em lógico, 200/scale x 100/scale.
            let p = placement(BackgroundImageMode::Center, frame, (200, 100), scale);
            let natural_w = 200.0 / scale;
            let natural_h = 100.0 / scale;
            assert!(!p.repeat);
            assert_rect_near(p.uv, r(0.0, 0.0, 1.0, 1.0), "a imagem inteira aparece");
            assert_rect_near(
                p.rect,
                r(
                    (400.0 - natural_w) / 2.0,
                    (200.0 - natural_h) / 2.0,
                    natural_w,
                    natural_h,
                ),
                &format!("centrada, escala {scale}"),
            );
        }
    }

    #[test]
    fn center_with_a_larger_image_is_cropped_keeping_the_middle() {
        let frame = r(0.0, 0.0, 100.0, 100.0);
        for scale in SCALES {
            // A imagem é 400x300 pixels físicos; o quadro, `100 * scale`.
            let p = placement(BackgroundImageMode::Center, frame, (400, 300), scale);
            // O desenho é o quadro inteiro...
            assert_rect_near(p.rect, frame, &format!("escala {scale}"));
            // ...mostrando a fração central da imagem, simétrica.
            let frame_px = 100.0 * scale;
            let shown_w = frame_px / 400.0;
            let shown_h = frame_px / 300.0;
            // Meio pixel da imagem (a menor, 300) em coordenada de textura.
            assert_rect_within(
                p.uv,
                r(
                    (1.0 - shown_w) / 2.0,
                    (1.0 - shown_h) / 2.0,
                    shown_w,
                    shown_h,
                ),
                0.6 / 300.0,
                &format!("uv, escala {scale}"),
            );
        }
    }

    /// O caso difícil: maior num eixo, menor no outro. O retângulo sai do
    /// quadro por um lado e não chega ao outro, e cada eixo é independente.
    #[test]
    fn center_with_the_image_wider_but_shorter_than_the_frame() {
        let frame = r(0.0, 0.0, 100.0, 100.0);
        for scale in SCALES {
            let frame_px = 100.0 * scale;
            // 500 pixels de largura (maior), 40 de altura (menor).
            let p = placement(BackgroundImageMode::Center, frame, (500, 40), scale);
            // Horizontal: o quadro inteiro, cortando a imagem dos dois lados.
            assert_rect_near(
                r(p.rect.x, 0.0, p.rect.width, 0.0),
                r(0.0, 0.0, 100.0, 0.0),
                &format!("x e largura, escala {scale}"),
            );
            let shown_w = frame_px / 500.0;
            assert_rect_within(
                r(p.uv.x, 0.0, p.uv.width, 0.0),
                r((1.0 - shown_w) / 2.0, 0.0, shown_w, 0.0),
                0.6 / 500.0,
                &format!("uv horizontal, escala {scale}"),
            );
            // Vertical: a imagem inteira (uv 0..1) no meio do quadro, a até
            // meio pixel físico (0.6 / scale em lógico).
            let natural_h = 40.0 / scale;
            assert_rect_within(
                r(0.0, p.rect.y, 0.0, p.rect.height),
                r(0.0, (100.0 - natural_h) / 2.0, 0.0, natural_h),
                0.6 / scale,
                &format!("y e altura, escala {scale}"),
            );
            assert_rect_near(
                r(0.0, p.uv.y, 0.0, p.uv.height),
                r(0.0, 0.0, 0.0, 1.0),
                &format!("uv vertical, escala {scale}"),
            );
        }
    }

    #[test]
    fn center_and_tile_start_on_a_whole_physical_pixel() {
        // Quadro em lógico com borda entre pixels: 10.3 * 1.25 = 12.875. A
        // origem do `center` tem de ser um pixel inteiro (o `rect` volta a
        // lógico dividindo por `scale`, e `scale * x` é inteiro).
        let frame = r(10.3, 7.7, 95.2, 60.1);
        for scale in SCALES {
            let p = placement(BackgroundImageMode::Center, frame, (30, 20), scale);
            for edge in [
                p.rect.x,
                p.rect.y,
                p.rect.x + p.rect.width,
                p.rect.y + p.rect.height,
            ] {
                let physical = edge * scale;
                assert!(
                    (physical - physical.round()).abs() < 1e-3,
                    "escala {scale}: borda {edge} cai em {physical}"
                );
            }
        }
    }

    #[test]
    fn center_keeps_the_image_pixel_for_pixel() {
        // Em qualquer escala, o `rect` físico tem exatamente o tamanho da
        // imagem quando ela cabe: um pixel da imagem por pixel da tela.
        let frame = r(0.0, 0.0, 300.0, 300.0);
        for scale in SCALES {
            let p = placement(BackgroundImageMode::Center, frame, (123, 77), scale);
            assert!(
                (p.rect.width * scale - 123.0).abs() < 1e-3,
                "escala {scale}"
            );
            assert!(
                (p.rect.height * scale - 77.0).abs() < 1e-3,
                "escala {scale}"
            );
        }
    }

    #[test]
    fn a_scale_change_only_changes_the_placement_not_the_image() {
        // O tamanho natural é `px / scale`: a mesma imagem ocupa menos pontos
        // lógicos em escala maior (RF-17.6), sem nova carga.
        let frame = r(0.0, 0.0, 400.0, 400.0);
        let at_1 = placement(BackgroundImageMode::Center, frame, (200, 200), 1.0);
        let at_2 = placement(BackgroundImageMode::Center, frame, (200, 200), 2.0);
        assert!((at_1.rect.width - 200.0).abs() < 1e-3);
        assert!((at_2.rect.width - 100.0).abs() < 1e-3);
        assert_eq!(at_1.uv, at_2.uv);
    }

    #[test]
    fn degenerate_inputs_fall_back_to_stretch() {
        let frame = r(0.0, 0.0, 100.0, 100.0);
        for (image, scale) in [
            ((0, 10), 1.0),
            ((10, 0), 1.0),
            ((10, 10), 0.0),
            ((10, 10), f32::NAN),
        ] {
            for mode in [BackgroundImageMode::Tile, BackgroundImageMode::Center] {
                let p = placement(mode, frame, image, scale);
                assert_eq!(p.rect, frame);
                assert_eq!(p.uv, r(0.0, 0.0, 1.0, 1.0));
                assert!(!p.repeat);
            }
        }
    }

    // ---- erros ---------------------------------------------------------

    #[test]
    fn a_missing_file_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nao-existe.png");
        assert_eq!(load_file(&path, 8192), Err(BackgroundImageError::NotFound));
        let (_, error) = BackgroundImageKey::probe(&path);
        assert_eq!(error, Some(BackgroundImageError::NotFound));
    }

    #[test]
    fn a_gif_is_unsupported_even_by_content() {
        // Cabeçalho de GIF89a, com extensão que mente (a leitura é por
        // conteúdo).
        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(&[1, 0, 1, 0, 0, 0, 0, 0x3b]);
        assert_eq!(
            decode_bytes(gif, 8192),
            Err(BackgroundImageError::UnsupportedFormat)
        );
    }

    #[test]
    fn garbage_and_empty_files_are_not_images() {
        assert_eq!(
            decode_bytes(b"isto nao e uma imagem".to_vec(), 8192),
            Err(BackgroundImageError::UnsupportedFormat)
        );
        assert_eq!(
            decode_bytes(Vec::new(), 8192),
            Err(BackgroundImageError::UnsupportedFormat)
        );
    }

    #[test]
    fn a_truncated_png_is_malformed() {
        let mut bytes = encode(&gradient(64, 64), ImageFormat::Png);
        bytes.truncate(bytes.len() / 2);
        assert_eq!(
            decode_bytes(bytes, 8192),
            Err(BackgroundImageError::Malformed)
        );
    }

    #[test]
    fn a_png_header_that_claims_a_huge_image_is_too_large() {
        // 100 000 x 100 000 RGBA8 = ~40 GB, contra o teto de 512 MiB dos
        // `Limits` padrão. PNG mínimo à mão: assinatura + IHDR + um IDAT (sem
        // ele o decodificador de PNG recusa o arquivo como malformado antes de
        // olhar o tamanho) + o fim, todos com CRC correto. O decodificador
        // recusa pelo cabeçalho, sem alocar.
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&100_000u32.to_be_bytes());
        ihdr.extend_from_slice(&100_000u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        push_chunk(&mut bytes, b"IHDR", &ihdr);
        push_chunk(
            &mut bytes,
            b"IDAT",
            &[0x78, 0x9c, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01],
        );
        push_chunk(&mut bytes, b"IEND", &[]);
        assert_eq!(
            decode_bytes(bytes, 8192),
            Err(BackgroundImageError::TooLarge)
        );
    }

    fn push_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc32(&body).to_be_bytes());
    }

    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = !0u32;
        for byte in bytes {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    #[test]
    fn a_directory_is_unreadable_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        let result = load_file(dir.path(), 8192);
        assert!(
            matches!(result, Err(BackgroundImageError::Unreadable(_))),
            "{result:?}"
        );
    }

    #[test]
    fn load_file_reads_a_real_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fundo.png");
        std::fs::write(&path, encode(&gradient(8, 8), ImageFormat::Png)).unwrap();
        let decoded = load_file(&path, 8192).unwrap();
        assert_eq!(decoded.size, (8, 8));
    }

    #[test]
    fn spawn_load_delivers_the_result_with_its_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fundo.png");
        std::fs::write(&path, encode(&gradient(8, 4), ImageFormat::Png)).unwrap();
        let (key, error) = BackgroundImageKey::probe(&path);
        assert_eq!(error, None);
        assert!(key.len > 0);

        let (tx, rx) = std::sync::mpsc::channel();
        spawn_load(key.clone(), 8192, move |result| {
            tx.send(result).unwrap();
        });
        let result = rx.recv_timeout(Duration::from_secs(10)).expect("resultado");
        assert_eq!(result.key, key);
        assert_eq!(result.outcome.unwrap().size, (8, 4));
    }

    #[test]
    fn spawn_load_of_a_missing_file_delivers_the_error() {
        let dir = tempfile::tempdir().unwrap();
        let key = BackgroundImageKey {
            path: dir.path().join("sumiu.png"),
            mtime: None,
            len: 0,
        };
        let (tx, rx) = std::sync::mpsc::channel();
        spawn_load(key, 8192, move |result| {
            tx.send(result).unwrap();
        });
        let result = rx.recv_timeout(Duration::from_secs(10)).expect("resultado");
        assert_eq!(result.outcome.unwrap_err(), BackgroundImageError::NotFound);
    }

    // ---- chave ---------------------------------------------------------

    #[test]
    fn the_key_changes_with_the_content_of_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fundo.png");
        std::fs::write(&path, b"um").unwrap();
        let (first, _) = BackgroundImageKey::probe(&path);
        let (same, _) = BackgroundImageKey::probe(&path);
        assert_eq!(first, same);
        std::fs::write(&path, b"dois bytes a mais").unwrap();
        let (changed, _) = BackgroundImageKey::probe(&path);
        assert_ne!(first, changed);
        assert_eq!(changed.len, 17);
    }

    // ---- descarte de resultado velho (função pura) ---------------------

    #[test]
    fn a_result_is_current_only_for_the_current_key() {
        let a = key("a.png", 1);
        let b = key("b.png", 1);
        assert!(result_is_current(Some(&a), &a));
        assert!(!result_is_current(Some(&b), &a));
        assert!(!result_is_current(None, &a));
        // Mesmo caminho, outro `mtime` ou tamanho: arquivo trocado no disco.
        assert!(!result_is_current(Some(&key("a.png", 2)), &a));
    }

    // ---- estado --------------------------------------------------------

    /// O handle de teste é um `u32`: `ImageId` é opaco, e o store não liga.
    type Store = BackgroundImageStore<u32>;

    /// O resultado de `key` chega e a textura `id` é criada.
    #[track_caller]
    fn finish(store: &mut Store, key: &BackgroundImageKey, id: u32) {
        assert_eq!(store.apply(loaded(key)), ApplyOutcome::Decoded);
        assert_eq!(store.upload(|_| Ok(id)), UploadOutcome::Ready);
    }

    fn ready(id: u32) -> BackgroundImageState<u32> {
        BackgroundImageState::Ready(BackgroundTexture { id, size: (1, 1) })
    }

    #[test]
    fn no_image_configured_stays_unchanged() {
        let mut store = Store::default();
        assert_eq!(store.sync(None), SyncOutcome::Unchanged);
        assert!(store.displayed().is_none());
    }

    #[test]
    fn a_new_key_loads_and_the_same_key_does_nothing() {
        let mut store = Store::default();
        let a = key("a.png", 1);
        assert_eq!(
            store.sync(Some((a.clone(), None))),
            SyncOutcome::Load(a.clone())
        );
        assert_eq!(store.state(), Some(&BackgroundImageState::Loading));
        // Mudar só `mode`/`opacity` refaz a conta e não a carga: mesma chave.
        assert_eq!(store.sync(Some((a.clone(), None))), SyncOutcome::Unchanged);
        assert_eq!(store.apply(loaded(&a)), ApplyOutcome::Decoded);
        // Os bytes esperam a textura; nada se desenha ainda.
        assert!(matches!(
            store.state(),
            Some(BackgroundImageState::Decoded(_))
        ));
        assert!(store.displayed().is_none());
        assert_eq!(store.upload(|_| Ok(9)), UploadOutcome::Ready);
        assert_eq!(store.sync(Some((a, None))), SyncOutcome::Unchanged);
        // Só o handle e o tamanho ficam: os bytes saíram do estado.
        assert_eq!(store.state(), Some(&ready(9)));
        assert_eq!(
            store.displayed(),
            Some(BackgroundTexture {
                id: 9,
                size: (1, 1)
            })
        );
        assert!(store.drain_released().is_empty());
    }

    #[test]
    fn upload_without_a_decoded_image_does_nothing() {
        let mut store = Store::default();
        assert_eq!(store.upload(|_| Ok(1)), UploadOutcome::Nothing);
        let a = key("a.png", 1);
        store.sync(Some((a, None)));
        // Ainda `Loading`: a closure nem é chamada.
        assert_eq!(
            store.upload(|_| panic!("não devia criar textura")),
            UploadOutcome::Nothing
        );
    }

    #[test]
    fn a_texture_the_gpu_refuses_is_a_failure_with_a_warning() {
        let mut store = Store::default();
        let a = key("a.png", 1);
        store.sync(Some((a.clone(), None)));
        store.apply(loaded(&a));
        let outcome = store.upload(|_| Err(BackgroundImageError::TooLarge));
        assert_eq!(
            outcome,
            UploadOutcome::Failed(BackgroundImageFailure {
                path: PathBuf::from("a.png"),
                error: BackgroundImageError::TooLarge,
            })
        );
        assert_eq!(
            store.state(),
            Some(&BackgroundImageState::Failed(
                BackgroundImageError::TooLarge
            ))
        );
        // Mesma chave na recarga seguinte: sem carga nova nem aviso novo.
        assert_eq!(store.sync(Some((a, None))), SyncOutcome::Unchanged);
    }

    #[test]
    fn a_result_with_an_old_key_is_discarded() {
        let mut store = Store::default();
        let a = key("a.png", 1);
        let b = key("b.png", 1);
        store.sync(Some((a.clone(), None)));
        store.sync(Some((b.clone(), None)));
        // A carga de `a` termina depois de `b` já ter sido pedida.
        assert_eq!(store.apply(loaded(&a)), ApplyOutcome::Discarded);
        assert_eq!(store.state(), Some(&BackgroundImageState::Loading));
        assert_eq!(store.apply(loaded(&b)), ApplyOutcome::Decoded);
        assert_eq!(store.key(), Some(&b));
    }

    #[test]
    fn a_failed_result_with_an_old_key_does_not_warn() {
        let mut store = Store::default();
        let a = key("a.png", 1);
        let b = key("b.png", 1);
        store.sync(Some((a.clone(), None)));
        store.sync(Some((b, None)));
        let stale = BackgroundImageResult {
            key: a,
            outcome: Err(BackgroundImageError::Malformed),
        };
        assert_eq!(store.apply(stale), ApplyOutcome::Discarded);
    }

    #[test]
    fn a_result_with_no_image_configured_is_discarded() {
        let mut store = Store::default();
        let a = key("a.png", 1);
        store.sync(Some((a.clone(), None)));
        assert_eq!(store.sync(None), SyncOutcome::Cleared);
        assert_eq!(store.apply(loaded(&a)), ApplyOutcome::Discarded);
        assert!(store.displayed().is_none());
    }

    #[test]
    fn the_previous_image_stays_until_the_new_one_is_ready() {
        let mut store = Store::default();
        let a = key("a.png", 1);
        let b = key("b.png", 1);
        store.sync(Some((a.clone(), None)));
        finish(&mut store, &a, 1);
        assert_eq!(
            store.sync(Some((b.clone(), None))),
            SyncOutcome::Load(b.clone())
        );
        // `b` ainda carrega: continua a textura de `a`, sem piscar sem imagem.
        assert_eq!(store.state(), Some(&BackgroundImageState::Loading));
        assert_eq!(store.displayed().map(|t| t.id), Some(1));
        // Uma terceira troca no meio da carga mantém a mesma "anterior".
        let c = key("c.png", 1);
        store.sync(Some((c.clone(), None)));
        assert_eq!(store.displayed().map(|t| t.id), Some(1));
        // Os bytes de `c` chegaram, mas a textura ainda não: segue a de `a`.
        assert_eq!(store.apply(loaded(&c)), ApplyOutcome::Decoded);
        assert_eq!(store.displayed().map(|t| t.id), Some(1));
        assert!(store.drain_released().is_empty());
        // A textura nova entra, e só então a de `a` é solta na GPU.
        assert_eq!(store.upload(|_| Ok(2)), UploadOutcome::Ready);
        assert_eq!(store.state(), Some(&ready(2)));
        assert_eq!(store.displayed().map(|t| t.id), Some(2));
        assert_eq!(store.drain_released(), vec![1]);
        assert!(store.drain_released().is_empty());
    }

    #[test]
    fn replacing_a_ready_texture_releases_exactly_the_old_one() {
        let mut store = Store::default();
        let a = key("a.png", 1);
        let b = key("b.png", 1);
        store.sync(Some((a.clone(), None)));
        finish(&mut store, &a, 10);
        store.sync(Some((b.clone(), None)));
        finish(&mut store, &b, 11);
        assert_eq!(store.drain_released(), vec![10]);
        // A de `b` segue viva.
        assert_eq!(store.displayed().map(|t| t.id), Some(11));
    }

    #[test]
    fn a_failure_removes_the_previous_image_and_warns_once() {
        let mut store = Store::default();
        let a = key("a.png", 1);
        let b = key("b.png", 1);
        store.sync(Some((a.clone(), None)));
        finish(&mut store, &a, 5);
        store.sync(Some((b.clone(), None)));
        let failed = BackgroundImageResult {
            key: b.clone(),
            outcome: Err(BackgroundImageError::UnsupportedFormat),
        };
        assert_eq!(
            store.apply(failed),
            ApplyOutcome::Failed(BackgroundImageFailure {
                path: PathBuf::from("b.png"),
                error: BackgroundImageError::UnsupportedFormat,
            })
        );
        assert!(store.displayed().is_none());
        // A textura anterior foi solta na GPU.
        assert_eq!(store.drain_released(), vec![5]);
        // Recarga com a mesma chave quebrada: nada, e portanto nenhum aviso
        // novo (RF-17.14: uma vez por arquivo e por problema).
        assert_eq!(store.sync(Some((b.clone(), None))), SyncOutcome::Unchanged);
        // Um resultado repetido para a mesma chave também não avisa de novo.
        let again = BackgroundImageResult {
            key: b,
            outcome: Err(BackgroundImageError::UnsupportedFormat),
        };
        assert_eq!(store.apply(again), ApplyOutcome::Discarded);
    }

    #[test]
    fn a_probe_failure_fails_at_once_and_warns_once() {
        let mut store = Store::default();
        let missing = BackgroundImageKey {
            path: PathBuf::from("nao-existe.png"),
            mtime: None,
            len: 0,
        };
        let outcome = store.sync(Some((
            missing.clone(),
            Some(BackgroundImageError::NotFound),
        )));
        assert_eq!(
            outcome,
            SyncOutcome::Failed(BackgroundImageFailure {
                path: PathBuf::from("nao-existe.png"),
                error: BackgroundImageError::NotFound,
            })
        );
        assert_eq!(
            store.state(),
            Some(&BackgroundImageState::Failed(
                BackgroundImageError::NotFound
            ))
        );
        // Mesmo caminho ainda ausente na recarga seguinte: sem aviso novo.
        let again = store.sync(Some((missing, Some(BackgroundImageError::NotFound))));
        assert_eq!(again, SyncOutcome::Unchanged);
    }

    #[test]
    fn the_file_appearing_after_a_failure_loads() {
        let mut store = Store::default();
        let missing = BackgroundImageKey {
            path: PathBuf::from("a.png"),
            mtime: None,
            len: 0,
        };
        store.sync(Some((missing, Some(BackgroundImageError::NotFound))));
        let present = key("a.png", 7);
        assert_eq!(
            store.sync(Some((present.clone(), None))),
            SyncOutcome::Load(present)
        );
    }

    #[test]
    fn clearing_the_path_drops_everything() {
        let mut store = Store::default();
        let a = key("a.png", 1);
        store.sync(Some((a.clone(), None)));
        finish(&mut store, &a, 3);
        assert_eq!(store.sync(None), SyncOutcome::Cleared);
        assert!(store.key().is_none());
        assert!(store.displayed().is_none());
        // A textura de `a` sai da GPU.
        assert_eq!(store.drain_released(), vec![3]);
        assert_eq!(store.sync(None), SyncOutcome::Unchanged);
    }

    #[test]
    fn clearing_while_a_new_image_loads_releases_the_previous_texture() {
        let mut store = Store::default();
        let a = key("a.png", 1);
        let b = key("b.png", 1);
        store.sync(Some((a.clone(), None)));
        finish(&mut store, &a, 4);
        store.sync(Some((b, None)));
        assert_eq!(store.sync(None), SyncOutcome::Cleared);
        assert_eq!(store.drain_released(), vec![4]);
        assert!(store.displayed().is_none());
    }

    #[test]
    fn a_probe_failure_releases_the_ready_texture() {
        let mut store = Store::default();
        let a = key("a.png", 1);
        store.sync(Some((a.clone(), None)));
        finish(&mut store, &a, 6);
        let gone = BackgroundImageKey {
            path: PathBuf::from("a.png"),
            mtime: None,
            len: 0,
        };
        let outcome = store.sync(Some((gone, Some(BackgroundImageError::NotFound))));
        assert!(matches!(outcome, SyncOutcome::Failed(_)));
        assert_eq!(store.drain_released(), vec![6]);
        assert!(store.displayed().is_none());
    }

    #[test]
    fn the_file_changing_on_disk_keeps_the_texture_until_the_new_one_is_up() {
        let mut store = Store::default();
        let before = key("a.png", 1);
        let after = key("a.png", 2); // mesmo caminho, outro mtime/tamanho
        store.sync(Some((before.clone(), None)));
        finish(&mut store, &before, 7);
        assert_eq!(
            store.sync(Some((after.clone(), None))),
            SyncOutcome::Load(after.clone())
        );
        assert_eq!(store.displayed().map(|t| t.id), Some(7));
        finish(&mut store, &after, 8);
        assert_eq!(store.drain_released(), vec![7]);
    }

    // ---- frases --------------------------------------------------------

    #[test]
    fn every_error_has_a_phrase_in_every_language() {
        use crate::messages::test_support;
        let failures = [
            BackgroundImageError::NotFound,
            BackgroundImageError::Unreadable(io::ErrorKind::PermissionDenied),
            BackgroundImageError::UnsupportedFormat,
            BackgroundImageError::Malformed,
            BackgroundImageError::TooLarge,
        ]
        .map(|error| BackgroundImageFailure {
            path: PathBuf::from("imagens/fundo.png"),
            error,
        });
        for locale in ["en_US", "pt_BR", "es_ES", "de_DE", "fr_FR"] {
            let catalog = test_support::catalog(locale);
            for failure in &failures {
                let (title, body) = failure.notice_text(&catalog);
                assert!(
                    !title.is_empty() && !title.contains("notice."),
                    "{locale} {title}"
                );
                assert!(body.contains("imagens/fundo.png"), "{locale}: {body}");
                assert!(!body.contains("notice."), "{locale}: {body}");
            }
        }
    }

    #[test]
    fn the_unsupported_phrase_names_the_accepted_formats() {
        let catalog = crate::messages::test_support::pt_br();
        let failure = BackgroundImageFailure {
            path: PathBuf::from("a.gif"),
            error: BackgroundImageError::UnsupportedFormat,
        };
        let (_, body) = failure.notice_text(&catalog);
        assert!(body.contains("PNG") && body.contains("JPEG"), "{body}");
    }
}
