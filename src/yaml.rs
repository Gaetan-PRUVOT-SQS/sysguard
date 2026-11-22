use crate::error::{Error, Result};

#[derive(Debug, PartialEq)]
pub enum Node {
    Scalar(String),
    Map(Vec<(String, Node)>),
    List(Vec<Node>),
}

pub fn cherche<'a>(carte: &'a Node, cle: &str) -> Option<&'a Node> {
    match carte {
        Node::Map(entries) => entries.iter().find(|(k, _)| k == cle).map(|(_, n)| n),
        _ => None,
    }
}

pub fn valeur_texte(noeud: &Node) -> &str {
    match noeud {
        Node::Scalar(s) => s,
        _ => "",
    }
}

pub fn parse(texte: &str) -> Result<Node> {
    let lignes = tokeniser(texte)?;
    let mut an = Analyse { lignes, pos: 0 };
    analyser_document(&mut an)
}

struct Ligne {
    indent: usize,
    texte: String,
}

struct Analyse {
    lignes: Vec<Ligne>,
    pos: usize,
}

fn erreur_ligne(pos: usize) -> Error {
    Error::new(format!("structure YAML invalide ligne {}", pos + 1))
}

fn tokeniser(texte: &str) -> Result<Vec<Ligne>> {
    let mut lignes = Vec::new();
    for ligne in texte.split('\n') {
        let indent = ligne.len() - ligne.trim_start_matches(' ').len();
        let debut = &ligne[indent..];
        if debut.is_empty() {
            continue;
        }
        if debut.starts_with('\t') {
            return Err(Error::new("indentation par tabulation"));
        }
        let fin = couper_commentaire(debut);
        let contenu = debut[..fin].trim_end_matches([' ', '\t', '\r']);
        if contenu.is_empty() {
            continue;
        }
        lignes.push(Ligne {
            indent,
            texte: contenu.to_string(),
        });
    }
    Ok(lignes)
}

fn couper_commentaire(ligne: &str) -> usize {
    let bytes = ligne.as_bytes();
    let mut guillemets = false;
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'"' {
            guillemets = !guillemets;
        } else if *b == b'#'
            && !guillemets
            && (i == 0 || bytes[i - 1] == b' ' || bytes[i - 1] == b'\t')
        {
            return i;
        }
    }
    ligne.len()
}

fn analyser_document(an: &mut Analyse) -> Result<Node> {
    if an.lignes.is_empty() {
        return Ok(Node::Map(Vec::new()));
    }
    let indent = an.lignes[0].indent;
    let mut racine = Node::Map(Vec::new());
    parse_bloc(&mut racine, an, indent)?;
    if an.pos < an.lignes.len() {
        return Err(erreur_ligne(an.pos));
    }
    Ok(racine)
}

fn parse_bloc(noeud: &mut Node, an: &mut Analyse, indent: usize) -> Result<()> {
    if an.pos >= an.lignes.len() || an.lignes[an.pos].indent < indent {
        return Ok(());
    }
    if an.lignes[an.pos].indent != indent {
        return Err(erreur_ligne(an.pos));
    }
    let texte = an.lignes[an.pos].texte.clone();
    if texte.starts_with('-') {
        if texte.as_bytes().get(1) != Some(&b' ') {
            return Err(Error::new(format!("element invalide ligne {}", an.pos + 1)));
        }
        *noeud = parse_liste(an, indent)?;
    } else {
        *noeud = parse_carte(an, indent)?;
    }
    Ok(())
}

fn parse_carte(an: &mut Analyse, indent: usize) -> Result<Node> {
    let mut entries = Vec::new();
    while an.pos < an.lignes.len()
        && an.lignes[an.pos].indent == indent
        && !an.lignes[an.pos].texte.starts_with('-')
    {
        let texte = an.lignes[an.pos].texte.clone();
        parse_entree(&mut entries, an, indent, &texte)?;
    }
    Ok(Node::Map(entries))
}

fn parse_entree(
    entries: &mut Vec<(String, Node)>,
    an: &mut Analyse,
    indent: usize,
    texte: &str,
) -> Result<()> {
    let (cle, rest) = extraire_cle(texte, an.pos)?;
    if entries.iter().any(|(k, _)| *k == cle) {
        return Err(Error::new(format!("cle dupliquee : {cle}")));
    }
    let noeud = scalaire_ou_bloc(an, indent, rest)?;
    entries.push((cle, noeud));
    Ok(())
}

fn extraire_cle(texte: &str, pos: usize) -> Result<(String, &str)> {
    let bytes = texte.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i] != b':' {
        if bytes[i] == b'"' {
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                i += 1;
            }
            if i >= bytes.len() {
                return Err(Error::new("guillemets non fermes"));
            }
        }
        i += 1;
    }
    if i >= bytes.len() {
        return Err(erreur_ligne(pos));
    }
    let cle = texte[..i].trim_end_matches([' ', '\t']);
    if cle.is_empty() {
        return Err(Error::new("cle vide"));
    }
    Ok((cle.to_string(), &texte[i + 1..]))
}

fn scalaire_ou_bloc(an: &mut Analyse, indent: usize, rest: &str) -> Result<Node> {
    let rest = rest.trim_start_matches([' ', '\t']);
    if !rest.is_empty() {
        an.pos += 1;
        return scalaire_texte(rest);
    }
    if an.pos + 1 < an.lignes.len() && an.lignes[an.pos + 1].indent > indent {
        an.pos += 1;
        let indent_enfant = an.lignes[an.pos].indent;
        let mut noeud = Node::Scalar(String::new());
        parse_bloc(&mut noeud, an, indent_enfant)?;
        return Ok(noeud);
    }
    an.pos += 1;
    Ok(Node::Scalar(String::new()))
}

fn scalaire_texte(rest: &str) -> Result<Node> {
    if rest == "[]" {
        return Ok(Node::List(Vec::new()));
    }
    let premier = rest.as_bytes()[0];
    if matches!(premier, b'&' | b'*' | b'|' | b'>') {
        let extrait: String = rest.chars().take(8).collect();
        return Err(Error::new(format!(
            "construction YAML non supportee : {extrait}"
        )));
    }
    if let Some(inner) = rest.strip_prefix('"') {
        let inner = inner
            .strip_suffix('"')
            .ok_or_else(|| Error::new("guillemets non fermes"))?;
        if inner.contains('\\') {
            return Err(Error::new("echappement non supporte"));
        }
        return Ok(Node::Scalar(inner.to_string()));
    }
    Ok(Node::Scalar(rest.to_string()))
}

fn parse_liste(an: &mut Analyse, indent: usize) -> Result<Node> {
    let mut items = Vec::new();
    while an.pos < an.lignes.len()
        && an.lignes[an.pos].indent == indent
        && an.lignes[an.pos].texte.starts_with('-')
        && an.lignes[an.pos].texte.as_bytes().get(1) == Some(&b' ')
    {
        let item = parse_item(an, indent)?;
        items.push(item);
    }
    Ok(Node::List(items))
}

fn parse_item(an: &mut Analyse, indent: usize) -> Result<Node> {
    let texte = an.lignes[an.pos].texte[2..]
        .trim_start_matches([' ', '\t'])
        .to_string();
    if !contient_deux_points(&texte) {
        an.pos += 1;
        return Ok(Node::Scalar(texte));
    }
    let mut entries = Vec::new();
    parse_entree(&mut entries, an, indent + 2, &texte)?;
    while an.pos < an.lignes.len()
        && an.lignes[an.pos].indent == indent + 2
        && !an.lignes[an.pos].texte.starts_with('-')
    {
        let suite = an.lignes[an.pos].texte.clone();
        parse_entree(&mut entries, an, indent + 2, &suite)?;
    }
    Ok(Node::Map(entries))
}

fn contient_deux_points(texte: &str) -> bool {
    let mut guillemets = false;
    for b in texte.bytes() {
        if b == b'"' {
            guillemets = !guillemets;
        } else if b == b':' && !guillemets {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analyser(texte: &str) -> Result<Node> {
        parse(texte)
    }

    #[test]
    fn racine_carte_et_entrees() {
        let doc = analyser("cle: valeur\nautre: \"avec : deux-points\"\n").expect("valide");
        assert!(matches!(&doc, Node::Map(e) if e.len() == 2));
        let autre = cherche(&doc, "autre").expect("autre présente");
        assert_eq!(valeur_texte(autre), "avec : deux-points");
    }

    #[test]
    fn cle_dupliquee_refusee() {
        assert!(analyser("a: 1\na: 2\n").is_err());
    }

    #[test]
    fn ligne_sans_cle_refusee() {
        assert!(analyser("pas-de-deux-points\n").is_err());
    }

    #[test]
    fn bloc_imbrique_carte() {
        let doc = analyser("global:\n  check_interval: 15s\n  log_level: info\n").expect("valide");
        let entries = match &doc {
            Node::Map(e) => e,
            _ => panic!("carte attendue"),
        };
        assert_eq!(entries.len(), 1);
        let global = cherche(&doc, "global").expect("global présente");
        assert!(matches!(global, Node::Map(e) if e.len() == 2));
        let intervalle = cherche(global, "check_interval").expect("champ");
        assert_eq!(valeur_texte(intervalle), "15s");
    }

    #[test]
    fn liste_de_cartes_et_scalaires() {
        let doc =
            analyser("rules:\n  - name: a\n    severity: info\n  - name: b\n").expect("valide");
        let rules = cherche(&doc, "rules").expect("rules");
        let items = match rules {
            Node::List(items) => items,
            _ => panic!("liste attendue"),
        };
        assert_eq!(items.len(), 2);
        assert!(matches!(&items[0], Node::Map(e) if e.len() == 2));
        let nom = cherche(&items[1], "name").expect("name");
        assert_eq!(valeur_texte(nom), "b");

        let doc = analyser("liste:\n  - un\n  - deux\n").expect("valide");
        let liste = cherche(&doc, "liste").expect("liste");
        assert!(matches!(liste, Node::List(items) if items.len() == 2));
        match liste {
            Node::List(items) => assert_eq!(valeur_texte(&items[1]), "deux"),
            _ => panic!("liste attendue"),
        }
    }

    #[test]
    fn liste_vide_flotte() {
        let doc = analyser("vide: []\n").expect("valide");
        let vide = cherche(&doc, "vide").expect("vide");
        assert!(matches!(vide, Node::List(items) if items.is_empty()));
    }

    #[test]
    fn refus_yaml() {
        let e = analyser("a:\n\tb: 1\n").expect_err("tabulation");
        assert!(e.to_string().contains("tabulation"));
        assert!(analyser("a: &ancre\n").is_err(), "ancre refusée");
        assert!(analyser("a: *etoile\n").is_err(), "alias refusé");
        assert!(analyser("a: \"cite\n").is_err(), "guillemets");
        let e = analyser("a: 1\nb: 2\n  c: 3\n").expect_err("orphelin");
        assert!(e.to_string().contains("ligne 3"));
        assert!(analyser("a: |lit\n").is_err(), "bloc pipe");
        assert!(analyser("a: >fold\n").is_err(), "bloc folded");
        assert!(analyser("a: \"a\\b\"\n").is_err(), "échappement");
        assert!(analyser(": valeur\n").is_err(), "cle vide");
        assert!(analyser("\"k: 1\n").is_err(), "guillemets en clé");
        assert!(analyser("a: \"\n").is_err(), "guillemet seul");
        assert!(analyser("a:\n  -x\n").is_err(), "tiret collé");
        assert!(
            analyser("a:\n  - x\n   - y\n").is_err(),
            "mauvais alignement"
        );
        assert!(analyser("a:\n  - x\n  -\n  - y\n").is_err(), "tiret seul");
    }

    #[test]
    fn desalignement_cite_ligne() {
        let e = analyser("a:\n  - x\n b: 2\n").expect_err("désalignement");
        assert!(e.to_string().contains("ligne 3"));
    }

    #[test]
    fn document_vide_carte_vide() {
        let doc = analyser("").expect("valide");
        assert!(matches!(&doc, Node::Map(e) if e.is_empty()));
    }

    #[test]
    fn commentaire_en_fin_de_ligne() {
        let doc = analyser("a: valeur # commentaire\n").expect("valide");
        let entries = match &doc {
            Node::Map(e) => e,
            _ => panic!("carte attendue"),
        };
        assert_eq!(valeur_texte(&entries[0].1), "valeur");
        assert!(cherche(&doc, "absente").is_none());
    }

    #[test]
    fn derniere_ligne_sans_saut() {
        assert!(analyser("a: 1").is_ok());
    }

    #[test]
    fn valeurs_limites() {
        let doc = analyser("a:\t# commentaire\n").expect("valide");
        match &doc {
            Node::Map(e) => assert_eq!(e.len(), 1),
            _ => panic!("carte attendue"),
        }
        let doc = analyser("a: 1\t# c\n").expect("valide");
        let entries = match &doc {
            Node::Map(e) => e,
            _ => panic!("carte attendue"),
        };
        assert_eq!(valeur_texte(&entries[0].1), "1");
        assert!(analyser("a: 1\r\n").is_ok(), "retour chariot");
        assert!(analyser("a: 1\t\n").is_ok(), "tabulation finale");
        let doc = analyser("a\t: 1\n").expect("valide");
        match &doc {
            Node::Map(e) => assert_eq!(e[0].0, "a"),
            _ => panic!("carte attendue"),
        }
        assert!(analyser("a:\t1\n").is_ok(), "tabulation après deux-points");
    }

    #[test]
    fn cle_vide_en_fin_et_cherche_dans_scalaire() {
        let doc = analyser("a:\n").expect("valide");
        match &doc {
            Node::Map(e) => assert_eq!(e.len(), 1),
            _ => panic!("carte attendue"),
        }
        let premier = match &doc {
            Node::Map(e) => &e[0].1,
            _ => panic!("carte attendue"),
        };
        assert!(cherche(premier, "x").is_none());
    }

    #[test]
    fn cle_citee_lue() {
        assert!(analyser("\"k\": 1\n").is_ok());
    }

    #[test]
    fn items_imbriques() {
        let doc = analyser("rules:\n  - c:\n      m: 1\n    a:\n      - t: log\n").expect("valide");
        let rules = cherche(&doc, "rules").expect("rules");
        let items = match rules {
            Node::List(items) => items,
            _ => panic!("liste attendue"),
        };
        assert!(matches!(&items[0], Node::Map(e) if e.len() == 2));
        assert!(analyser("a:\n  - \ty\n").is_ok(), "tabulation après tiret");
        assert!(analyser("rules:\n  -  \"x: 1\"\n").is_ok(), "item cité");
        assert!(analyser("a:\n  - x\n").is_ok(), "item simple");
    }

    #[test]
    fn valeur_commentaire_non_coupé_dans_guillemets() {
        let doc = analyser("a: \"v # x\"\n").expect("valide");
        let entries = match &doc {
            Node::Map(e) => e,
            _ => panic!("carte attendue"),
        };
        assert_eq!(valeur_texte(&entries[0].1), "v # x");
    }

    #[test]
    fn cherche_dans_carte_inexistante() {
        let doc = analyser("a: 1\n").expect("valide");
        assert!(cherche(&doc, "b").is_none());
    }

    #[test]
    fn aleatoire_entree_hostile_ne_panique_jamais() {
        // Technique aléatoire (29119-4) : graine fixe, profil d'octets YAML.
        // Propriété : parse ne panique pas et termine ; tout est Ok ou Err.
        let mut graine: u64 = 20_261_122;
        let mut suivant = move || {
            graine ^= graine << 13;
            graine ^= graine >> 7;
            graine ^= graine << 17;
            graine
        };
        let alphabet: &[u8] = b"azAZ:-#[]\"\t 019\n{}|>&*!%";
        let mut acceptes = 0;
        for _ in 0..500 {
            let longueur = (suivant() % 64) as usize;
            let texte: String = (0..longueur)
                .map(|_| alphabet[(suivant() as usize) % alphabet.len()] as char)
                .collect();
            if parse(&texte).is_ok() {
                acceptes += 1;
            }
        }
        assert!(acceptes > 0, "le profil doit couvrir des entrées valides");
    }

    #[test]
    fn metamorphique_commentaire_invariant() {
        // Ajouter une ligne de commentaire ne change pas la structure.
        let base = "global:\n  check_interval: 15s\n  log_level: info\nrules:\n  - name: a\n    condition:\n      metric: cpu.usage\n      operator: \">\"\n      value: 1\n    actions:\n      - type: log\n";
        let avec = "# un commentaire\n# un autre\n".to_string() + base + "# fin\n";
        assert_eq!(parse(base).expect("base"), parse(&avec).expect("avec"));
    }
}
