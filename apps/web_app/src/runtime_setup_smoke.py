"""Fictional runtime qualification only. No employer requests or model inference."""
import json
import os
from pathlib import Path
import re
import socket
import subprocess
import sys
import time
import urllib.request
import zipfile


def prepare(folder):
    body = '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>'
    for text in ['Fictional Candidate', 'Systems Engineer', 'Professional profile',
                 'Engineering systems, reliable software and clear documentation.']:
        body += '<w:p><w:r><w:t>' + text + '</w:t></w:r></w:p>'
    body += '<w:sectPr><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="1440" w:bottom="1440" w:left="1440" w:right="1440"/></w:sectPr></w:body></w:document>'
    with zipfile.ZipFile(folder / 'fictional.docx', 'w', zipfile.ZIP_DEFLATED) as archive:
        archive.writestr('[Content_Types].xml', '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>')
        archive.writestr('_rels/.rels', '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>')
        archive.writestr('word/document.xml', body)
    return {'fictional_docx': True}


def pdf(folder):
    import pypdf
    import pypdfium2
    path = folder / 'fictional.pdf'
    reader = pypdf.PdfReader(path)
    text = '\n'.join(page.extract_text() or '' for page in reader.pages)
    for expected in ['Fictional Candidate', 'Systems Engineer', 'Professional profile']:
        if expected not in text:
            raise ValueError('Converted PDF lost expected fictional text')
    if len(reader.pages) != 1:
        raise ValueError('Unexpected fictional PDF page count')
    with pypdfium2.PdfDocument(path) as doc:
        page = doc[0]
        bitmap = page.render(scale=1)
        image = bitmap.to_pil()
        image.save(folder / 'fictional.png')
        image.close()
        bitmap.close()
        page.close()
    return {'pages': len(reader.pages), 'expected_text_present': True, 'rendered': True}


def ollama(folder, binary, expected_version):
    profile = folder / 'isolated-user'
    environment = os.environ.copy()
    for name, relative in {'USERPROFILE':'user', 'APPDATA':'roaming', 'LOCALAPPDATA':'local',
                           'TEMP':'temp', 'TMP':'temp', 'OLLAMA_MODELS':'models'}.items():
        target = profile / relative
        target.mkdir(parents=True, exist_ok=True)
        environment[name] = str(target)
    with socket.socket() as reservation:
        reservation.bind(('127.0.0.1', 0))
        port = reservation.getsockname()[1]
    environment.update(OLLAMA_HOST=f'127.0.0.1:{port}', OLLAMA_MAX_LOADED_MODELS='1',
                       OLLAMA_NUM_PARALLEL='1', OLLAMA_NO_CLOUD='1')
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    def read(endpoint):
        with opener.open(f'http://127.0.0.1:{port}'+endpoint, timeout=1) as response:
            data = response.read(65537)
            if response.status != 200 or len(data) > 65536:
                raise ValueError('Private API response exceeded its bounds')
            return json.loads(data)
    log = folder / 'ollama-server.private.log'
    with log.open('xb') as output:
        child = subprocess.Popen([str(binary), 'serve'], env=environment, stdin=subprocess.DEVNULL,
                                 stdout=output, stderr=subprocess.STDOUT,
                                 creationflags=subprocess.CREATE_NO_WINDOW)
        try:
            deadline = time.monotonic() + 30
            while True:
                if child.poll() is not None:
                    raise ValueError('Private Ollama exited before readiness')
                if log.stat().st_size > 2 * 1024**2:
                    raise ValueError('Private Ollama exceeded its log budget')
                try:
                    version = read('/api/version')
                    break
                except (OSError, ValueError):
                    if time.monotonic() >= deadline:
                        raise ValueError('Private Ollama startup timed out')
                    time.sleep(.2)
            if version.get('version') != expected_version:
                raise ValueError('Private server version differs from the pinned package')
            if read('/api/tags').get('models') != [] or read('/api/ps').get('models') != []:
                raise ValueError('The qualification must not adopt or load any model')
            if child.poll() is not None:
                raise ValueError('Private server exited during API checks')
            return {'version': version['version'], 'model_count': 0, 'resident_model_count': 0,
                    'profile_isolated': True, 'pid': child.pid, 'port': port}
        finally:
            if child.poll() is None:
                child.terminate()
                try:
                    child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait(timeout=5)


if __name__ == '__main__':
    mode, raw_folder, *arguments = sys.argv[1:]
    folder = Path(raw_folder).resolve(strict=True)
    result = prepare(folder) if mode == 'prepare' else pdf(folder) if mode == 'pdf' else ollama(folder, Path(arguments[0]), arguments[1]) if mode == 'ollama' else None
    if result is None:
        raise ValueError('Unknown qualification mode')
    print(json.dumps(result))
