---
title: Einen vollständigen Bericht setzen
subtitle: Gliederung, Nummerierung und Quellen
author: [Erika Mustermann, Max Beispiel]
date: 7. Oktober 2026
abstract: |
  Dieser Bericht entsteht aus einer einzigen Markdown-Datei. Titelseite, Inhaltsverzeichnis, nummerierte Überschriften, Kopfzeilen und Seitenzahlen kommen aus dem Thema, und jeder Verweis auf einen Abschnitt, eine Abbildung, eine Tabelle oder eine Seite wird erzeugt.

  Der Bericht verbindet außerdem Spalten, Abbildungen, Tabellen, Fußnoten und ein Literaturverzeichnis. Die erzeugten Nummern und Quellenangaben müssen zu einem Satz passen, der sich während des Schreibens ständig ändert.
lang: de
title-page: true
toc: true
bibliography: references.bib
---

::: page-break

# Einleitung {#sec:einleitung}

Ein Bericht ist mehr als eine Folge von Absätzen. Der Leser erwartet eine Titelseite, ein Inhaltsverzeichnis, das mit den Seiten übereinstimmt, der Reihe nach nummerierte Überschriften und Verweise, die an die richtige Stelle führen. Wird eines davon von Hand gesetzt, zerbricht es bei der nächsten Änderung. Kyber erzeugt alles aus dem Text und aus dem Thema, sodass der Autor `@sec:tabellen` schreibt und im Satz „@sec:tabellen“ steht.

Das Handwerk hinter diesen Erwartungen ist alt und gut beschrieben. @tschichold1928 forderte schon 1928 eine klare, sachliche Ordnung der Seite, @bringhurst2004 beschreibt Proportion und Rhythmus als Aufgabe des Setzers, und @forssman2002 sammeln die Einzelheiten, die im Satz täglich zu entscheiden sind.

Die folgenden Abschnitte folgen der Reihenfolge, in der ein Leser die Teile eines Berichts kennenlernt. @sec:spalten setzt Text in zwei Spalten mit Anmerkungen aus beiden. @sec:abbildungen enthält die Abbildungen, und @sec:tabellen beginnt mit einer kurzen Tabelle und endet mit einer langen, die sich über mehrere Seiten erstreckt. @sec:verweise erklärt, wie die Verweise in diesem Text aufgelöst werden und warum man den Seitenzahlen darin trauen kann.[^vertrauen] @sec:quellen zeigt, wie die Werke hinter dem Text zitiert werden.

[^vertrauen]: Die Seitenzahlen stammen aus dem endgültigen Satz. @sec:verweise auf [@sec:verweise, page] beschreibt, wie sie sich einpendeln.

## Was der Leser zuerst sieht

Die Titelseite trägt Titel, Untertitel, Autoren, Datum und Zusammenfassung, jedes in einem eigenen Feld des Themas. Ein Feld ohne Wert entfällt samt dem Abstand darüber, ein Bericht ohne Untertitel braucht also kein anderes Thema. Das Inhaltsverzeichnis steht auf einer eigenen Seite, der Haupttext beginnt auf der Seite danach. Wie Leser solche Seiten auf Papier und am Bildschirm wahrnehmen, behandeln die Aufsätze in @hartmann2015.

Jede Textseite hat eine Kopfzeile mit dem Titel des Abschnitts und eine Fußzeile mit der Seitenzahl. Die Seiten werden ab der Titelseite gezählt, die Zahl in der Fußzeile ist also die Zahl, die ein PDF-Betrachter für die Seite anzeigt.

# Spalten {#sec:spalten}

Ein Bericht enthält oft Stoff, den der Leser eher überfliegt als liest: Begriffslisten, kurze Anmerkungen zu Quellen oder eine Zusammenfassung der Ergebnisse. Zwei Spalten eignen sich dafür. Der folgende Abschnitt schaltet auf zwei Spalten um, läuft über einen Seitenumbruch und kehrt an seinem Ende zu einer Spalte zurück.

::: columns
## Lesefolge

In einem zweispaltigen Abschnitt läuft der Text zuerst die erste Spalte hinunter, dann die zweite und danach auf der nächsten Seite weiter. Wer am Fuß der zweiten Spalte angekommen ist, blättert um und liest oben in der ersten Spalte weiter. Die Suche nach Seitenumbrüchen behandelt die beiden Spalten einer Seite als einen Bereich, dessen Höhe die der höheren Spalte ist. Ein Abschnitt kann daher mitten auf einer Seite beginnen und mitten auf einer anderen enden. Den Ausgleich der Spalten gegen Fußnoten und Tabellen untersuchen @okafor2022 [S. 104-107], und für Bücher mit großen Tabellen vergleicht @schaefer2018 [S. 40-52] mehrere Satzverfahren.[^reihenfolge]

[^reihenfolge]: Anmerkungen werden in der Lesefolge nummeriert. Eine Anmerkung aus der ersten Spalte steht also vor einer aus der zweiten. Dass Anmerkungen am Seitenfuß stehen, obwohl sie am Ende einer Quelle geschrieben werden, kennt man schon aus den Programmen von @knuth1984lit.

Jede Spalte ist schmaler als der Satzspiegel, ihre Zeilen fassen weniger Wörter, und die Silbentrennung kommt häufiger vor. Der Absatzumbruch setzt jede Zeile einer Spalte in der Spaltenbreite und wählt die Umbrüche für den ganzen Absatz auf einmal, genau wie bei Text über die volle Breite. Das Verfahren stammt von @knuthplass1981, die Trennmuster folgen @liang1983, und wo eine Zeile überhaupt umbrochen werden darf, regeln die Vorschriften von @unicode16.

## Anmerkungen in Spalten

Fußnoten gehören keiner Spalte. Sie teilen sich unten auf der Seite einen Bereich über die ganze Breite, unter einer kurzen Linie, gleich wo ihre Verweise stehen.[^geteilt] Eine lange Anmerkung kann auf der nächsten Seite unter ihrer Nummer und dem Hinweis „Fortsetzung“ weiterlaufen.

[^geteilt]: Eine Seite mit zwei Spaltenabschnitten und einem Block über die volle Breite dazwischen hat dennoch nur einen Anmerkungsbereich, und seine Anmerkungen bleiben in der Reihenfolge ihrer Verweise.

::: full-width
![Ein Balkendiagramm über die volle Breite, zwischen zwei Spaltenbereichen.](images/chart.svg){#fig:breit}
:::

Die Abbildung oben, @fig:breit, unterbricht die Spalten. Die Spalten davor werden ausgeglichen, die Abbildung läuft über den Satzspiegel, und darunter setzen die Spalten wieder ein. Ein Verweis darauf liest sich an jeder Stelle des Berichts gleich.

Der Ausgleich wählt den Spaltenumbruch, der beide Spalten am gleichmäßigsten macht. Würde der gleichmäßigste Umbruch eine einzelne Zeile verwaisen lassen, gilt stattdessen ein Umbruch, der um eine Zeile weniger gleichmäßig ist. Ein Spaltenbereich wird nie weiter gedehnt, als es für den Abstand zwischen Blöcken überhaupt gilt.
:::

Der Abschnitt endet mit einem Absatz über die volle Breite. Seine erste Zeile beginnt unterhalb der höheren der beiden Spalten darüber.

# Abbildungen {#sec:abbildungen}

Abbildungen werden in der Reihenfolge ihres Erscheinens über den ganzen Bericht gezählt. Das Diagramm in @sec:spalten ist @fig:breit. Die beiden Abbildungen in diesem Abschnitt setzen die Zählung fort, und ihre Marken erlauben es dem Text, vor oder nach ihrem Erscheinen auf sie zu verweisen.

![Eine Landschaft als kleines Rasterbild.](images/landscape.png){#fig:landschaft}

@fig:landschaft ist ein PNG-Bild von 240 mal 150 Pixeln und steht in seiner natürlichen Größe. @fig:turm unten ist eine hohe Zeichnung, die auf die Höhe schrumpft, die neben ihrer Bildunterschrift bleibt.

![Eine hohe Zeichnung, die samt Bildunterschrift auf die Seite passt.](images/tower.svg){#fig:turm}

Beide Zeichnungen gelangen als Zeichenoperatoren in die PDF-Datei, deren Format @adobe2006 beschreibt und das als [@iso32000] genormt ist. Ein Bild behält seinen Platz im Text, denn es gibt keine Gleitobjekte. Passt es nicht in den Rest einer Seite, wandert es mit seiner Bildunterschrift auf die nächste, und die Seite, die es verlässt, endet kurz.

# Tabellen {#sec:tabellen}

Tabellen werden getrennt von den Abbildungen gezählt. @tbl:einstellungen ist kurz und behält ihre natürliche Breite. @tbl:geschichte ist lang: Sie beginnt auf [@tbl:geschichte, page] und wiederholt ihre Kopfzeile auf jeder Seite, auf der sie weiterläuft.

| Einstellung | Standard | Wirkung |
|:--|:-:|:--|
| `numbered-headings` | true | Nummeriert Überschriften bis `numbering-depth`. |
| `toc` | false | Setzt vor den Haupttext ein Inhaltsverzeichnis. |
| `title-page` | false | Setzt den Titel auf eine eigene Seite. |

: Dokumenteinstellungen für die Gliederung eines Berichts. {#tbl:einstellungen}

| Jahr | Ereignis | Anmerkung |
|--:|:--|:--|
| 1439 | Bewegliche Lettern in Mainz | Gutenberg verbindet Gießinstrument, Druckfarbe auf Ölbasis und Spindelpresse. |
| 1455 | Die 42-zeilige Bibel | Etwa 180 Exemplare, einige auf Pergament. Jede Seite ist in zwei Spalten zu 42 Zeilen gesetzt. |
| 1470 | Jensons Antiqua | Eine in Venedig geschnittene Antiqua, die bis heute als Vorbild für Buchschriften dient. |
| 1495 | *De Aetna* | Aldus Manutius druckt Bembos Bericht über die Besteigung des Ätna in einer neuen Antiqua. |
| 1501 | Kursive | Die erste Kursive, von Francesco Griffo für kleine, handliche Ausgaben der Klassiker geschnitten. |
| 1530 | Garamond | Claude Garamonds Antiqua verbreitet sich im französischen und dann im europäischen Buchhandel. |
| 1557 | Civilité | Robert Granjon schneidet eine Schrift nach dem Vorbild der französischen Handschrift. |
| 1692 | Romain du Roi | Eine Kommission der Pariser Akademie der Wissenschaften entwirft eine Antiqua auf feinem Raster. |
| 1734 | Caslon | William Caslons Schriftprobe. |
| 1757 | Baskervilles Vergil | Velinpapier, schwarze Farbe und scharfer Kontrast. |
| 1784 | Didot-Punkt | François-Ambroise Didot legt den Punkt fest, in dem französische und deutsche Drucker Schrift messen. |
| 1798 | Klassizistische Antiqua | Bodoni und Didot treiben den Kontrast auf die Spitze: haarfeine Serifen und senkrechte Achse. |
| 1814 | Schnellpresse | Die Times wird von Koenig und Bauer auf einer dampfgetriebenen Zylinderpresse gedruckt. |
| 1816 | Serifenlose | Caslon IV zeigt die erste serifenlose Schrift, nur in Versalien. |
| 1845 | Rotationspresse | Richard Hoes Rotationspresse legt die Lettern auf einen Zylinder. |
| 1886 | Linotype | Ottmar Mergenthalers Maschine gießt ganze Zeilen aus Messingmatrizen, die an einer Tastatur gesetzt werden.[^linotype] |
| 1887 | Monotype | Tolbert Lanston trennt Tastatur und Gießmaschine: Ein Lochstreifen steuert den Guss einzelner Buchstaben. |
| 1896 | Kelmscott Chaucer | William Morris druckt Chaucer mit Holzschnittrahmen. |
| 1916 | Johnston Sans | Edward Johnstons Alphabet für die Londoner U-Bahn. |
| 1927 | Futura | Paul Renners geometrische Groteskschrift. |
| 1928 | *Die neue Typographie* | Jan Tschichold fordert asymmetrische Seiten und Groteskschriften und kehrt später zur klassischen Buchtypografie zurück. |
| 1932 | Times New Roman | Für die gleichnamige Zeitung entworfen, mit schmalen Proportionen, um in Spalten Platz zu sparen. |
| 1949 | Fotosatz | Die Lumitype projiziert Buchstaben von einer Scheibe auf Film. |
| 1957 | Univers und Helvetica | Zwei Groteskschriften in einem Jahr; Univers plant von Beginn an eine Familie aus 21 Schnitten. |
| 1965 | Digitale Schrift | Der Digiset zeichnet Buchstaben aus gespeicherten Umrissen auf einer Kathodenstrahlröhre. |
| 1978 | TeX | Donald Knuth beginnt TeX, um den zweiten Band von *The Art of Computer Programming* zu setzen. |
| 1984 | PostScript | Eine Seitenbeschreibungssprache, die Schrift als Umrisse behandelt. |
| 1985 | Desktop-Publishing | LaserWriter und PageMaker bringen den Satz auf den Schreibtisch. |
| 1993 | PDF | Portable Document Format, gebaut auf dem Bildmodell von PostScript. |
| 1996 | OpenType | Ein Schriftformat für beide Umrissarten, mit Tabellen für Ligaturen, Unterschneidung und Varianten. |
| 2010 | WOFF | Schriften für das Web. |

: Fünf Jahrhunderte Schrift und Druck. {#tbl:geschichte}

[^linotype]: Eine Fußnote, auf die in einer Tabellenzelle verwiesen wird, steht auf der Seite ihrer Zeile wie jede andere.

Der Text geht nach der Tabelle weiter. Die Überschrift von @tbl:geschichte bleibt bei ihren ersten Zeilen, und die Kopfzeile wird oben auf jeder Seite, auf der sie weiterläuft, erneut gesetzt. Die Zeilen folgen der Geschichte, wie sie [@tschichold1928; @bringhurst2004] erzählen, und die Zeile für 1978 meint das Programm, das @knuth1984 beschreibt. Dass eine Schrift, die nur die Zeichen eines Dokuments enthält, die Dateien klein hält, haben @idocs2019 gemessen.

# Querverweise {#sec:verweise}

Eine Marke benennt eine Überschrift, eine Abbildung oder eine Tabelle. Sie beginnt mit der Art des Benannten: `sec:` für eine Überschrift, `fig:` für eine Abbildung und `tbl:` für eine Tabelle. Ein Verweis schreibt die Marke nach einem At-Zeichen und zeigt die Bezeichnung der Art und die Nummer, wie bei @fig:landschaft oder @tbl:einstellungen. In Klammern mit `page` zeigt er stattdessen die Seite: Die lange Tabelle beginnt auf [@tbl:geschichte, page], und die hohe Zeichnung steht auf [@fig:turm, page]. Jeder Verweis ist ein Link auf sein Ziel.

## Seitenzahlen, die sich einpendeln

Ein Seitenverweis ist Text, und seine Breite kann einen Zeilenumbruch verschieben, einen Seitenumbruch und damit die Seite, auf die er verweist. Das Inhaltsverzeichnis hat dasselbe Problem: Seine Länge bestimmt, wo der Haupttext beginnt. Kyber setzt den Bericht, liest die Seiten aller Überschriften, Abbildungen und Tabellen und setzt ihn mit diesen Zahlen erneut, bis sie sich nicht mehr ändern. Die meisten Dokumente brauchen zwei Durchläufe. Ein Dokument, dessen Zahlen immer weiterwandern, wird gemeldet und nicht mit falschen Zahlen geschrieben.

## Überschriften ohne Nummer

Eine Überschrift unterhalb der Nummerierungstiefe hat keine Nummer, und ein Verweis darauf zeigt ihren Text. Das Standardthema nummeriert drei Ebenen, die Überschrift der vierten Ebene unten hat also keine, und ein Verweis darauf liest sich: @sec:ruhig.

#### Eine ruhige Überschrift {#sec:ruhig}

Diese Überschrift steht auf der vierten Ebene. Im Inhaltsverzeichnis, das bis `toc-depth` reicht, erscheint sie nicht.

# Quellen {#sec:quellen}

Zitate funktionieren wie Querverweise. Ein At-Zeichen und ein Schlüssel aus der Datei, die `bibliography` im Kopf des Dokuments nennt, zitieren ein Werk. In Klammern, wie in `[@schluessel]`, steht das Zitat in runden Klammern, mehrere Werke trennt ein Semikolon. Ohne Klammern wird das Werk Teil des Satzes. Eine Seitenangabe folgt dem Schlüssel nach einem Komma in den Klammern, etwa `[@schluessel, S. 12]` oder `[@schluessel, S. 3-5]`, und bei einem Zitat im Satz in eckigen Klammern nach einem Leerzeichen.

Gruppierte Zitate passen ans Ende einer Aussage, die mehrere Werke stützen [@knuthplass1981; @liang1983; @okafor2022]. Ein Zitat im Satz passt zu einer Aussage über ein einzelnes Werk. Die Aufsatzsammlung @hartmann2015 [S. 31-35] zeigt, wie eine Seitenspanne aussieht, und die Studie @koenig2021 [S. 41] zeigt eine einzelne Seite. Zur Lesbarkeit am Bildschirm, zu Zeilenlänge, Durchschuss und Schriftgröße, äußern sich @koenig2021 [S. 36-44] ausführlich.

Auch eine Anmerkung darf zitieren.[^zitat] Die Abschnitte oben zitieren innerhalb der zwei Spalten von @sec:spalten, im Text nach der langen Tabelle von @sec:tabellen und in den Anmerkungen.

[^zitat]: In einer Anmerkung kann man zum Beispiel auf den Standard verweisen, den @unicode16 festlegt, oder auf das Handbuch von @forssman2002, das Regeln für Zahlen, Striche und Anführungszeichen aufführt.

# Schluss {#sec:schluss}

Ein so gesetzter Bericht bleibt stimmig, während er sich ändert. Wird @sec:abbildungen vor @sec:spalten gesetzt, ändern sich die Nummern beider Abschnitte, ihrer Abbildungen, des Inhaltsverzeichnisses und jedes Verweises, und die Seitenverweise folgen dem neuen Satz. Dasselbe gilt für das Literaturverzeichnis, das den Zitaten folgt. Es steht unten in zwei Spalten, wird erzeugt und enthält nur die Werke, die im Text zitiert sind.

::: columns
::: bibliography
:::
