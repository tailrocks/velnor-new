export class ArchiveAdmissionError extends Error {
  constructor(code, message) {
    super(`${code}: ${message}`);
    this.name = 'ArchiveAdmissionError';
    this.code = code;
  }
}

export function admissionFail(code, message) {
  throw new ArchiveAdmissionError(code, message);
}
