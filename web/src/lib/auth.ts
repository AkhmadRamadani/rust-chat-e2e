export const auth = {
  getAdminToken: () => typeof window !== 'undefined' ? sessionStorage.getItem('adminToken') : null,
  setAdminToken: (token: string) => typeof window !== 'undefined' && sessionStorage.setItem('adminToken', token),
  clearAdminToken: () => typeof window !== 'undefined' && sessionStorage.removeItem('adminToken'),

  getRegistrationCredentials: () => {
    if (typeof window === 'undefined') return null;
    const id = sessionStorage.getItem('reg_id');
    const token = sessionStorage.getItem('reg_token');
    return id && token ? { id, token } : null;
  },
  setRegistrationCredentials: (id: string, token: string) => {
    if (typeof window !== 'undefined') {
      sessionStorage.setItem('reg_id', id);
      sessionStorage.setItem('reg_token', token);
    }
  },
  clearRegistrationCredentials: () => {
    if (typeof window !== 'undefined') {
      sessionStorage.removeItem('reg_id');
      sessionStorage.removeItem('reg_token');
    }
  }
};
